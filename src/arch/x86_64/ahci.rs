//! Driver AHCI polling. O adaptador de bloco mantém o I/O serializado e
//! respeita o limite de setores reportado pelo IDENTIFY.

use carlio_os::{errors::KernelError, pci::PciFunction};
use core::{
    arch::asm,
    ptr,
    sync::atomic::{compiler_fence, Ordering},
};

const PCI_ADDRESS: u16 = 0xcf8;
const PCI_DATA: u16 = 0xcfc;
const HBA_PORTS: usize = 0x100 / 4;
const PORT_STRIDE: usize = 0x80 / 4;
const PORT_CMD: usize = 0x18 / 4;
const PORT_TFD: usize = 0x20 / 4;
const PORT_SIG: usize = 0x24 / 4;
const PORT_SSTS: usize = 0x28 / 4;
const PORT_SCTL: usize = 0x2c / 4;
const PORT_SERR: usize = 0x30 / 4;
const PORT_CI: usize = 0x38 / 4;
const CMD_ST: u32 = 1;
const CMD_FRE: u32 = 1 << 4;
const CMD_FR: u32 = 1 << 14;
const CMD_CR: u32 = 1 << 15;
const TFD_BSY_DRQ: u32 = (1 << 7) | (1 << 3);
const HBA_IS_TFES: u32 = 1 << 30;
const POLL_LIMIT: usize = 10_000_000;
const PAGE_PRESENT: u64 = 1;
const PAGE_WRITE: u64 = 1 << 1;
const PAGE_PWT: u64 = 1 << 3;
const PAGE_PCD: u64 = 1 << 4;
const PAGE_LARGE: u64 = 1 << 7;

#[repr(C, align(4096))]
struct PageTable([u64; 512]);

static mut MMIO_PDPT: PageTable = PageTable([0; 512]);
static mut MMIO_PD: PageTable = PageTable([0; 512]);

#[repr(C, align(1024))]
struct DmaMemory {
    command_list: [u8; 1024],
    received_fis: [u8; 256],
    command_table: [u8; 256],
    identify: [u8; 512],
    sector: [u8; 512],
}

static mut DMA: DmaMemory = DmaMemory {
    command_list: [0; 1024],
    received_fis: [0; 256],
    command_table: [0; 256],
    identify: [0; 512],
    sector: [0; 512],
};

#[derive(Clone, Copy)]
struct Controller {
    port: *mut u32,
    map: AddressMap,
    #[cfg_attr(not(feature = "storage-test"), allow(dead_code))]
    sectors: u64,
}

static mut CONTROLLER: Option<Controller> = None;
static BLOCK_IO_LOCK: carlio_os::sync::IrqSpinLock<()> = carlio_os::sync::IrqSpinLock::new(());

/// Dispositivo de bloco AHCI descoberto pelo kernel.
/// Não executa I/O durante a construção; o chamador escolhe quando montar ou
/// escrever. Proteção contra uso simultâneo é compartilhada pelo HBA.
pub struct AhciBlockDevice {
    map: AddressMap,
    sectors: u64,
}

pub fn block_device() -> Result<AhciBlockDevice, KernelError> {
    let controller = unsafe { CONTROLLER }.ok_or(KernelError::DeviceNotFound)?;
    if controller.sectors == 0 {
        return Err(KernelError::DeviceNotFound);
    }
    Ok(AhciBlockDevice {
        map: controller.map,
        sectors: controller.sectors,
    })
}

impl carlio_os::block::BlockDevice for AhciBlockDevice {
    fn sector_count(&self) -> u64 {
        self.sectors
    }

    fn read_sector(
        &mut self,
        lba: u64,
        out: &mut [u8; carlio_os::hermione::SECTOR_SIZE],
    ) -> Result<(), carlio_os::block::BlockError> {
        if lba >= self.sectors {
            return Err(carlio_os::block::BlockError::OutOfRange);
        }
        let _guard = BLOCK_IO_LOCK.lock();
        command(&self.map, 0x25, lba, false, false, true).map_err(map_block_error)?;
        let source = unsafe { core::ptr::addr_of!(DMA.sector).cast::<u8>() };
        for (index, byte) in out.iter_mut().enumerate() {
            *byte = unsafe { ptr::read_volatile(source.add(index)) };
        }
        Ok(())
    }

    fn write_sector(
        &mut self,
        lba: u64,
        data: &[u8; carlio_os::hermione::SECTOR_SIZE],
    ) -> Result<(), carlio_os::block::BlockError> {
        if lba >= self.sectors {
            return Err(carlio_os::block::BlockError::OutOfRange);
        }
        let _guard = BLOCK_IO_LOCK.lock();
        let target = unsafe { core::ptr::addr_of_mut!(DMA.sector).cast::<u8>() };
        for (index, byte) in data.iter().enumerate() {
            unsafe { ptr::write_volatile(target.add(index), *byte) };
        }
        compiler_fence(Ordering::SeqCst);
        command(&self.map, 0x35, lba, true, false, true).map_err(map_block_error)
    }

    fn flush(&mut self) -> Result<(), carlio_os::block::BlockError> {
        let _guard = BLOCK_IO_LOCK.lock();
        command(&self.map, 0xea, 0, false, false, false).map_err(map_block_error)
    }
}

fn map_block_error(error: KernelError) -> carlio_os::block::BlockError {
    match error {
        KernelError::IoTimeout => carlio_os::block::BlockError::Timeout,
        KernelError::DeviceNotFound => carlio_os::block::BlockError::Removed,
        _ => carlio_os::block::BlockError::Device,
    }
}

#[derive(Clone, Copy)]
struct AddressMap {
    hhdm: u64,
    kernel_physical: u64,
    kernel_virtual: u64,
}

impl AddressMap {
    fn dma(&self, virtual_address: u64) -> Result<u64, KernelError> {
        let offset = virtual_address
            .checked_sub(self.kernel_virtual)
            .ok_or(KernelError::DeviceInitFailed)?;
        self.kernel_physical
            .checked_add(offset)
            .ok_or(KernelError::DeviceInitFailed)
    }

    fn mmio(&self, physical_address: u64) -> Result<*mut u32, KernelError> {
        let virtual_address = self
            .hhdm
            .checked_add(physical_address)
            .ok_or(KernelError::DeviceInitFailed)?;
        if virtual_address & 3 != 0 {
            return Err(KernelError::DeviceInitFailed);
        }
        unsafe { map_mmio(self, virtual_address, physical_address)? };
        Ok(virtual_address as *mut u32)
    }
}

/// Mapeia um BAR PCI de memória usando o mesmo HHDM/pagetables dos drivers.
/// O chamador habilita memória/bus mastering após validar o dispositivo.
pub unsafe fn map_pci_bar(
    function: PciFunction,
    bar_offset: u8,
    hhdm: u64,
    kernel_physical: u64,
    kernel_virtual: u64,
) -> Result<*mut u8, KernelError> {
    if bar_offset < 0x10 || bar_offset > 0x24 || bar_offset & 3 != 0 {
        return Err(KernelError::UnsupportedDevice);
    }
    let map = AddressMap {
        hhdm,
        kernel_physical,
        kernel_virtual,
    };
    let low = pci_read(function, bar_offset);
    if low & 1 != 0 || low & 0xffff_fff0 == 0 {
        return Err(KernelError::DeviceInitFailed);
    }
    let bar_type = (low >> 1) & 3;
    let physical = match bar_type {
        0 => (low & !0xf) as u64,
        2 if bar_offset <= 0x20 => {
            ((pci_read(function, bar_offset + 4) as u64) << 32) | (low as u64 & !0xf)
        }
        _ => return Err(KernelError::UnsupportedDevice),
    };
    Ok(map.mmio(physical)?.cast::<u8>())
}

unsafe fn map_mmio(
    map: &AddressMap,
    virtual_address: u64,
    physical_address: u64,
) -> Result<(), KernelError> {
    let cr3: u64;
    unsafe { asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags)) };
    let root = map
        .hhdm
        .checked_add(cr3 & !0xfff)
        .ok_or(KernelError::DeviceInitFailed)? as *mut u64;
    let pml4_index = ((virtual_address >> 39) & 0x1ff) as usize;
    let pdpt_index = ((virtual_address >> 30) & 0x1ff) as usize;
    let pd_index = ((virtual_address >> 21) & 0x1ff) as usize;
    let pt_index = ((virtual_address >> 12) & 0x1ff) as usize;
    let pml4e = unsafe { ptr::read_volatile(root.add(pml4_index)) };
    let pdpt = if pml4e & PAGE_PRESENT == 0 {
        let table_phys = map.dma(core::ptr::addr_of!(MMIO_PDPT) as u64)?;
        unsafe { ptr::write_bytes(core::ptr::addr_of_mut!(MMIO_PDPT.0).cast::<u8>(), 0, 4096) };
        unsafe {
            ptr::write_volatile(root.add(pml4_index), table_phys | PAGE_PRESENT | PAGE_WRITE)
        };
        (map.hhdm + table_phys) as *mut u64
    } else {
        (map.hhdm + (pml4e & !0xfff)) as *mut u64
    };
    let pdpte = unsafe { ptr::read_volatile(pdpt.add(pdpt_index)) };
    let pd = if pdpte & PAGE_PRESENT == 0 {
        let table_phys = map.dma(core::ptr::addr_of!(MMIO_PD) as u64)?;
        unsafe { ptr::write_bytes(core::ptr::addr_of_mut!(MMIO_PD.0).cast::<u8>(), 0, 4096) };
        unsafe {
            ptr::write_volatile(pdpt.add(pdpt_index), table_phys | PAGE_PRESENT | PAGE_WRITE)
        };
        (map.hhdm + table_phys) as *mut u64
    } else if pdpte & PAGE_LARGE != 0 {
        return Ok(());
    } else {
        (map.hhdm + (pdpte & !0xfff)) as *mut u64
    };
    let pde = unsafe { ptr::read_volatile(pd.add(pd_index)) };
    if pde & PAGE_PRESENT == 0 {
        let physical_2m = physical_address & !0x1f_ffff;
        unsafe {
            ptr::write_volatile(
                pd.add(pd_index),
                physical_2m | PAGE_PRESENT | PAGE_WRITE | PAGE_PWT | PAGE_PCD | PAGE_LARGE,
            )
        };
    } else if pde & PAGE_LARGE == 0 {
        let pt = (map.hhdm + (pde & !0xfff)) as *mut u64;
        let pte = unsafe { ptr::read_volatile(pt.add(pt_index)) };
        if pte & PAGE_PRESENT == 0 {
            let physical_4k = physical_address & !0xfff;
            unsafe {
                ptr::write_volatile(
                    pt.add(pt_index),
                    physical_4k | PAGE_PRESENT | PAGE_WRITE | PAGE_PWT | PAGE_PCD,
                )
            };
        }
    }
    unsafe {
        asm!("invlpg [{}]", in(reg) virtual_address, options(nostack, preserves_flags));
    }
    Ok(())
}

pub unsafe fn initialize(
    function: PciFunction,
    hhdm: u64,
    kernel_physical: u64,
    kernel_virtual: u64,
) -> Result<u64, KernelError> {
    if !matches!(
        function.ahci_model(),
        Some(
            carlio_os::pci::AhciModel::IntelQemu
                | carlio_os::pci::AhciModel::IntelNas
                | carlio_os::pci::AhciModel::IntelRstLocal
        )
    ) {
        return Err(KernelError::UnsupportedDevice);
    }
    let map = AddressMap {
        hhdm,
        kernel_physical,
        kernel_virtual,
    };
    let command = pci_read(function, 0x04) as u16;
    pci_write(function, 0x04, (command | (1 << 1) | (1 << 2)) as u32);

    let bar_low = pci_read(function, 0x24);
    if bar_low & 1 != 0 || bar_low & 0xffff_fff0 == 0 {
        return Err(KernelError::DeviceInitFailed);
    }
    let bar_type = (bar_low >> 1) & 3;
    let bar = if bar_type == 2 {
        ((pci_read(function, 0x28) as u64) << 32) | (bar_low as u64 & !0xf)
    } else if bar_type == 0 {
        (bar_low & !0xf) as u64
    } else {
        return Err(KernelError::UnsupportedDevice);
    };
    let hba = map.mmio(bar)?;
    if unsafe { read_reg(hba, 0) } & (1 << 31) == 0 {
        return Err(KernelError::UnsupportedDevice);
    }
    let ghc = unsafe { read_reg(hba, 1) } | (1 << 31);
    unsafe { write_reg(hba, 1, ghc) };
    let implemented = unsafe { read_reg(hba, 3) };

    for port_number in 0..32 {
        if implemented & (1 << port_number) == 0 {
            continue;
        }
        let port = unsafe { hba.add(HBA_PORTS + port_number * PORT_STRIDE) };
        let ssts = unsafe { read_reg(port, PORT_SSTS) };
        let signature = unsafe { read_reg(port, PORT_SIG) };
        if ssts & 0xf != 3 || (ssts >> 8) & 0xf != 1 || signature != 0x0000_0101 {
            continue;
        }
        prepare_port(port, &map)?;
        unsafe {
            CONTROLLER = Some(Controller {
                port,
                sectors: 0,
                map,
            })
        };
        identify(&map)?;
        let sectors = identify_capacity();
        unsafe { CONTROLLER = Some(Controller { port, sectors, map }) };
        return Ok(sectors);
    }
    Err(KernelError::DeviceNotFound)
}

#[cfg(feature = "storage-test")]
pub unsafe fn disposable_disk_test() -> Result<(), KernelError> {
    let controller = unsafe { CONTROLLER }.ok_or(KernelError::DeviceNotFound)?;
    if controller.sectors < 2 {
        return Err(KernelError::DeviceNotFound);
    }
    let map = unsafe {
        let hhdm = core::ptr::addr_of!(super::HHDM_OFFSET).read_volatile();
        let kernel_physical = core::ptr::addr_of!(super::KERNEL_PHYSICAL_BASE).read_volatile();
        let kernel_virtual = core::ptr::addr_of!(super::KERNEL_VIRTUAL_BASE).read_volatile();
        AddressMap {
            hhdm,
            kernel_physical,
            kernel_virtual,
        }
    };
    unsafe { read_write_sector(&map, 1, true)? };
    unsafe { read_write_sector(&map, 1, false)? };
    let sector = unsafe { core::ptr::addr_of!(DMA.sector).cast::<u8>() };
    for index in 0..512 {
        if unsafe { ptr::read_volatile(sector.add(index)) } != test_byte(index) {
            return Err(KernelError::ControllerError);
        }
    }
    match command(&map, 0xff, 0, false, false, false) {
        Err(KernelError::ControllerError) => Ok(()),
        Err(error) => Err(error),
        Ok(()) => Err(KernelError::ControllerError),
    }
}

#[cfg(feature = "storage-test")]
pub unsafe fn disposable_filesystem_test() -> Result<(), KernelError> {
    use carlio_os::hermione::HermioneVolume;

    let _controller = unsafe { CONTROLLER }.ok_or(KernelError::DeviceNotFound)?;
    let disk = block_device()?;
    super::write("Hermione: formatando superbloco/diretorio...\r\n");
    let mut volume = HermioneVolume::format(disk).map_err(|_| KernelError::ControllerError)?;
    let expected = b"Hermione persistiu no disco QEMU descartavel.";
    super::write("Hermione: gravando arquivo...\r\n");
    volume
        .write_file("teste.txt", expected)
        .map_err(|_| KernelError::ControllerError)?;
    let disk = volume.into_device();
    super::write("Hermione: remontando volume...\r\n");
    let mut volume = HermioneVolume::mount(disk).map_err(|_| KernelError::ControllerError)?;
    let mut output = [0; 64];
    super::write("Hermione: lendo e validando checksum...\r\n");
    let count = volume
        .read_file("teste.txt", &mut output)
        .map_err(|_| KernelError::ControllerError)?;
    if &output[..count] != expected {
        return Err(KernelError::ControllerError);
    }
    let scrub = volume.scrub().map_err(|_| KernelError::ControllerError)?;
    if scrub.files_verified != 1 || scrub.copies_repaired != 0 {
        return Err(KernelError::ControllerError);
    }
    Ok(())
}

fn prepare_port(port: *mut u32, map: &AddressMap) -> Result<(), KernelError> {
    let mut command = unsafe { read_reg(port, PORT_CMD) };
    command &= !(CMD_ST | CMD_FRE);
    unsafe { write_reg(port, PORT_CMD, command) };
    if !poll_clear(port, PORT_CMD, CMD_CR | CMD_FR) {
        return Err(KernelError::IoTimeout);
    }
    let clb = unsafe { core::ptr::addr_of!(DMA.command_list) as u64 };
    let fb = unsafe { core::ptr::addr_of!(DMA.received_fis) as u64 };
    let clb = map.dma(clb)?;
    let fb = map.dma(fb)?;
    if clb & 0x3ff != 0 || fb & 0xff != 0 {
        return Err(KernelError::DeviceInitFailed);
    }
    unsafe {
        write_reg(port, 0, clb as u32);
        write_reg(port, 1, (clb >> 32) as u32);
        write_reg(port, 2, fb as u32);
        write_reg(port, 3, (fb >> 32) as u32);
        write_reg(port, PORT_SERR, u32::MAX);
        write_reg(port, 0x10 / 4, u32::MAX);
        write_reg(port, PORT_CMD, command | CMD_FRE);
    }
    if !poll_set(port, PORT_CMD, CMD_FR) {
        return Err(KernelError::IoTimeout);
    }
    unsafe { write_reg(port, PORT_CMD, command | CMD_FRE | CMD_ST) };
    if !poll_set(port, PORT_CMD, CMD_CR) {
        return Err(KernelError::IoTimeout);
    }
    Ok(())
}

fn identify(map: &AddressMap) -> Result<(), KernelError> {
    command(map, 0xec, 0, false, true, true)
}

#[cfg(feature = "storage-test")]
unsafe fn read_write_sector(map: &AddressMap, lba: u64, write: bool) -> Result<(), KernelError> {
    if write {
        for index in 0..512 {
            unsafe {
                ptr::write_volatile(core::ptr::addr_of_mut!(DMA.sector[index]), test_byte(index))
            };
        }
        compiler_fence(Ordering::SeqCst);
    }
    command(
        map,
        if write { 0x35 } else { 0x25 },
        lba,
        write,
        false,
        true,
    )
}

fn command(
    map: &AddressMap,
    opcode: u8,
    lba: u64,
    write: bool,
    identify: bool,
    transfer_data: bool,
) -> Result<(), KernelError> {
    let controller = unsafe { CONTROLLER }.ok_or(KernelError::DeviceNotFound)?;
    let table = unsafe { core::ptr::addr_of_mut!(DMA.command_table).cast::<u8>() };
    let data = unsafe {
        if identify {
            core::ptr::addr_of_mut!(DMA.identify)
        } else {
            core::ptr::addr_of_mut!(DMA.sector)
        }
    };
    unsafe { ptr::write_bytes(table, 0, 256) };
    let data_phys = if transfer_data {
        map.dma(data as u64)?
    } else {
        0
    };
    let table_phys = map.dma(table as u64)?;
    let list = unsafe { core::ptr::addr_of_mut!(DMA.command_list).cast::<u8>() };
    unsafe { ptr::write_bytes(list, 0, 1024) };

    let flags: u16 = 5 | if write { 1 << 6 } else { 0 };
    unsafe {
        ptr::write_volatile(list.cast::<u16>(), flags);
        ptr::write_volatile(list.add(2).cast::<u16>(), u16::from(transfer_data));
        ptr::write_volatile(list.add(8).cast::<u32>(), table_phys as u32);
        ptr::write_volatile(list.add(12).cast::<u32>(), (table_phys >> 32) as u32);
        ptr::write_volatile(table, 0x27);
        ptr::write_volatile(table.add(1), 0x80);
        ptr::write_volatile(table.add(2), opcode);
        ptr::write_volatile(table.add(4), lba as u8);
        ptr::write_volatile(table.add(5), (lba >> 8) as u8);
        ptr::write_volatile(table.add(6), (lba >> 16) as u8);
        ptr::write_volatile(table.add(7), if identify { 0 } else { 0x40 });
        ptr::write_volatile(table.add(8), (lba >> 24) as u8);
        ptr::write_volatile(table.add(9), (lba >> 32) as u8);
        ptr::write_volatile(table.add(10), (lba >> 40) as u8);
        ptr::write_volatile(table.add(12), 1);
        if transfer_data {
            ptr::write_volatile(table.add(128).cast::<u32>(), data_phys as u32);
            ptr::write_volatile(table.add(132).cast::<u32>(), (data_phys >> 32) as u32);
            ptr::write_volatile(table.add(140).cast::<u32>(), 511);
        }
    }
    compiler_fence(Ordering::SeqCst);
    unsafe { write_reg(controller.port, 0x30 / 4, u32::MAX) };
    if !poll_clear(controller.port, PORT_TFD, TFD_BSY_DRQ) {
        return Err(KernelError::IoTimeout);
    }
    unsafe { write_reg(controller.port, PORT_CI, 1) };
    for iteration in 0..POLL_LIMIT {
        if iteration & 0x3fff == 0 {
            super::idt::heartbeat();
        }
        let is = unsafe { read_reg(controller.port, 0x10 / 4) };
        if is & HBA_IS_TFES != 0 {
            recover_port(controller.port, map)?;
            return Err(KernelError::ControllerError);
        }
        if unsafe { read_reg(controller.port, PORT_CI) } & 1 == 0 {
            compiler_fence(Ordering::SeqCst);
            return Ok(());
        }
        core::hint::spin_loop();
    }
    Err(KernelError::IoTimeout)
}

/// Reinicia somente o link SATA após erro de task file, preservando o HBA.
/// COMRESET segue a sequência e a temporização mínima da AHCI 1.3.1.
fn recover_port(port: *mut u32, map: &AddressMap) -> Result<(), KernelError> {
    let command = unsafe { read_reg(port, PORT_CMD) } & !CMD_ST;
    unsafe { write_reg(port, PORT_CMD, command) };
    if !poll_clear(port, PORT_CMD, CMD_CR) {
        #[cfg(feature = "storage-test")]
        super::write("AHCI recovery: timeout parando command engine.\r\n");
        return Err(KernelError::IoTimeout);
    }

    let command = unsafe { read_reg(port, PORT_CMD) } & !CMD_FRE;
    unsafe { write_reg(port, PORT_CMD, command) };
    if !poll_clear(port, PORT_CMD, CMD_FR) {
        #[cfg(feature = "storage-test")]
        super::write("AHCI recovery: timeout parando FIS receive engine.\r\n");
        return Err(KernelError::IoTimeout);
    }

    let sctl = unsafe { read_reg(port, PORT_SCTL) };
    unsafe { write_reg(port, PORT_SCTL, (sctl & !0xf) | 1) };
    if !super::idt::wait_ticks(2) {
        #[cfg(feature = "storage-test")]
        super::write("AHCI recovery: PIT nao avancou durante COMRESET.\r\n");
        return Err(KernelError::IoTimeout);
    }
    unsafe { write_reg(port, PORT_SCTL, sctl & !0xf) };

    let mut link_ready = false;
    for iteration in 0..POLL_LIMIT {
        if unsafe { read_reg(port, PORT_SSTS) } & 0xf == 3 {
            link_ready = true;
            break;
        }
        if iteration & 0x3fff == 0 {
            super::idt::heartbeat();
        }
        core::hint::spin_loop();
    }
    if !link_ready {
        #[cfg(feature = "storage-test")]
        super::write("AHCI recovery: link nao voltou apos COMRESET.\r\n");
        return Err(KernelError::DeviceNotFound);
    }

    unsafe {
        write_reg(port, PORT_SERR, u32::MAX);
        write_reg(port, 0x10 / 4, u32::MAX);
    }
    let prepared = prepare_port(port, map);
    #[cfg(feature = "storage-test")]
    if prepared.is_err() {
        super::write("AHCI recovery: falha ao reiniciar command engine.\r\n");
    }
    prepared
}

fn poll_clear(base: *mut u32, register: usize, mask: u32) -> bool {
    for iteration in 0..POLL_LIMIT {
        if unsafe { read_reg(base, register) } & mask == 0 {
            return true;
        }
        if iteration & 0x3fff == 0 {
            super::idt::heartbeat();
        }
        core::hint::spin_loop();
    }
    false
}

fn poll_set(base: *mut u32, register: usize, mask: u32) -> bool {
    for iteration in 0..POLL_LIMIT {
        if unsafe { read_reg(base, register) } & mask == mask {
            return true;
        }
        if iteration & 0x3fff == 0 {
            super::idt::heartbeat();
        }
        core::hint::spin_loop();
    }
    false
}

fn identify_capacity() -> u64 {
    let identify = unsafe { core::ptr::addr_of!(DMA.identify) };
    let words = identify.cast::<u16>();
    let lba48 = unsafe {
        (ptr::read_volatile(words.add(103)) as u64) << 48
            | (ptr::read_volatile(words.add(102)) as u64) << 32
            | (ptr::read_volatile(words.add(101)) as u64) << 16
            | ptr::read_volatile(words.add(100)) as u64
    };
    if lba48 != 0 {
        lba48
    } else {
        unsafe {
            ((ptr::read_volatile(words.add(61)) as u64) << 16)
                | ptr::read_volatile(words.add(60)) as u64
        }
    }
}

#[cfg(feature = "storage-test")]
fn test_byte(index: usize) -> u8 {
    (index as u8).wrapping_mul(37).wrapping_add(0x5a)
}

unsafe fn read_reg(base: *mut u32, register: usize) -> u32 {
    unsafe { ptr::read_volatile(base.add(register)) }
}

unsafe fn write_reg(base: *mut u32, register: usize, value: u32) {
    unsafe { ptr::write_volatile(base.add(register), value) }
}

fn pci_read(function: PciFunction, offset: u8) -> u32 {
    let address = 0x8000_0000
        | ((function.bus as u32) << 16)
        | ((function.device as u32) << 11)
        | ((function.function as u32) << 8)
        | (offset as u32 & 0xfc);
    unsafe {
        asm!("out dx, eax", in("dx") PCI_ADDRESS, in("eax") address, options(nomem, nostack, preserves_flags));
        let value: u32;
        asm!("in eax, dx", in("dx") PCI_DATA, out("eax") value, options(nomem, nostack, preserves_flags));
        value
    }
}

fn pci_write(function: PciFunction, offset: u8, value: u32) {
    let address = 0x8000_0000
        | ((function.bus as u32) << 16)
        | ((function.device as u32) << 11)
        | ((function.function as u32) << 8)
        | (offset as u32 & 0xfc);
    unsafe {
        asm!("out dx, eax", in("dx") PCI_ADDRESS, in("eax") address, options(nomem, nostack, preserves_flags));
        asm!("out dx, eax", in("dx") PCI_DATA, in("eax") value, options(nomem, nostack, preserves_flags));
    }
}

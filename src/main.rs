#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

#[cfg(target_os = "none")]
use carlio_os::errors::{ErrorLog, KernelError};
#[cfg(target_os = "none")]
use carlio_os::heap::KernelHeapSet;
#[cfg(target_os = "none")]
use carlio_os::memory::{
    FrameError, OwnedAddressSpace, PageTableAccess, PageTableManager, PhysicalFrameAllocator,
    PAGE_FLAG_NO_EXECUTE, PAGE_FLAG_USER, PAGE_FLAG_WRITABLE, PHYSICAL_FRAME_BITMAP_WORDS,
};
#[cfg(target_os = "none")]
use carlio_os::pci::PciFunction;
#[cfg(target_os = "none")]
use carlio_os::process::{CpuContext, KernelStackPool, ReapBarrier, Scheduler};
#[cfg(target_os = "none")]
use carlio_os::sync::IrqSpinLock;
#[cfg(target_os = "none")]
use carlio_os::vfs::{AccessContext, Credentials, Vfs};
#[cfg(target_os = "none")]
use carlio_os::{parse_command, Command};
#[cfg(target_os = "none")]
use core::arch::asm;
#[cfg(target_os = "none")]
use core::arch::global_asm;
#[cfg(target_os = "none")]
use core::fmt::{self, Write as FmtWrite};
#[cfg(target_os = "none")]
use core::panic::PanicInfo;
#[cfg(target_os = "none")]
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

#[cfg(target_os = "none")]
const COM1: u16 = 0x3f8;
#[cfg(target_os = "none")]
const INPUT_LIMIT: usize = 128;
#[cfg(target_os = "none")]
// Leituras concorrentes em APs podem disputar o único byte injetado pelo
// harness; um timeout curto mantém serial best-effort sem prender o scheduler.
const SERIAL_POLL_LIMIT: usize = 100_000;
#[cfg(target_os = "none")]
const SERIAL_READ_POLL_LIMIT: usize = 250_000;
#[cfg(target_os = "none")]
static SERIAL_RX_OWNER: carlio_os::serial_rx::SerialRxGate =
    carlio_os::serial_rx::SerialRxGate::new();
#[cfg(target_os = "none")]
const ERROR_LOG_CAPACITY: usize = 16;
#[cfg(target_os = "none")]
const MAX_BOOT_CPUS: usize = 64;
#[cfg(target_os = "none")]
const AP_STACK_SIZE: usize = 16 * 1024;

#[cfg(target_os = "none")]
static mut ERROR_LOG: ErrorLog<ERROR_LOG_CAPACITY> = ErrorLog::new();
#[cfg(target_os = "none")]
static mut HHDM_OFFSET: u64 = 0;
#[cfg(target_os = "none")]
static mut KERNEL_PHYSICAL_BASE: u64 = 0;
#[cfg(target_os = "none")]
static mut KERNEL_VIRTUAL_BASE: u64 = 0;
#[cfg(target_os = "none")]
static FRAME_ALLOCATOR: IrqSpinLock<PhysicalFrameAllocator<PHYSICAL_FRAME_BITMAP_WORDS>> =
    IrqSpinLock::new(PhysicalFrameAllocator::new());
#[cfg(target_os = "none")]
static PROCESS_SCHEDULER: Scheduler<64, MAX_BOOT_CPUS> = Scheduler::new();
#[cfg(target_os = "none")]
static PROCESS_KERNEL_STACKS: KernelStackPool<64, 16_384> = KernelStackPool::new();
#[cfg(target_os = "none")]
static NETWORK_SOCKETS: IrqSpinLock<carlio_os::net::SocketTable<128>> =
    IrqSpinLock::new(carlio_os::net::SocketTable::new());
#[cfg(target_os = "none")]
const PROCESS_VFS_CAPACITY: usize = 40;
#[cfg(target_os = "none")]
static PROCESS_VFS: IrqSpinLock<Vfs<PROCESS_VFS_CAPACITY>> = IrqSpinLock::new(Vfs::new());
#[cfg(target_os = "none")]
static ROOTFS_VFS_STAGING: IrqSpinLock<Vfs<PROCESS_VFS_CAPACITY>> = IrqSpinLock::new(Vfs::new());
#[cfg(target_os = "none")]
static ELF_LOAD_SCRATCH: IrqSpinLock<[u8; carlio_os::vfs::VFS_INLINE_FILE_MAX]> =
    IrqSpinLock::new([0; carlio_os::vfs::VFS_INLINE_FILE_MAX]);
#[cfg(target_os = "none")]
static ELF_INTERPRETER_SCRATCH: IrqSpinLock<[u8; carlio_os::vfs::VFS_INLINE_FILE_MAX]> =
    IrqSpinLock::new([0; carlio_os::vfs::VFS_INLINE_FILE_MAX]);
#[cfg(target_os = "none")]
static OPEN_FILES: IrqSpinLock<carlio_os::open_files::OpenFileTable<128>> =
    IrqSpinLock::new(carlio_os::open_files::OpenFileTable::new());
#[cfg(target_os = "none")]
static AHCI_BLOCK_DEVICE: IrqSpinLock<Option<ahci::AhciBlockDevice>> = IrqSpinLock::new(None);
#[cfg(target_os = "none")]
static PERSISTENT_ROOTFS: IrqSpinLock<
    Option<carlio_os::hermione::HermioneVolume<ahci::AhciBlockDevice>>,
> = IrqSpinLock::new(None);
#[cfg(target_os = "none")]
static ROOTFS_TRANSACTION_LOCK: IrqSpinLock<()> = IrqSpinLock::new(());
#[cfg(target_os = "none")]
const ROOTFS_SNAPSHOT_CAPACITY: usize =
    20 + PROCESS_VFS_CAPACITY * (18 + 31 + carlio_os::vfs::VFS_INLINE_FILE_MAX);
#[cfg(target_os = "none")]
static ROOTFS_SNAPSHOT: IrqSpinLock<[u8; ROOTFS_SNAPSHOT_CAPACITY]> =
    IrqSpinLock::new([0; ROOTFS_SNAPSHOT_CAPACITY]);
#[cfg(target_os = "none")]
static PROCESS_PROBE_IDS: [AtomicU64; 3] =
    [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];
#[cfg(target_os = "none")]
static PROCESS_PROBE_SWITCHES: AtomicU64 = AtomicU64::new(0);
#[cfg(target_os = "none")]
static PROCESS_PROBE_CR3_SWITCHES: AtomicU64 = AtomicU64::new(0);
#[cfg(target_os = "none")]
static PROCESS_KERNEL_CR3: AtomicU64 = AtomicU64::new(0);
#[cfg(target_os = "none")]
static PROCESS_REAP_BARRIER: ReapBarrier<MAX_BOOT_CPUS> = ReapBarrier::new();
#[cfg(target_os = "none")]
static AP_CPL3_CPU_MASK: AtomicUsize = AtomicUsize::new(0);
#[cfg(target_os = "none")]
static AP_CPL3_RUNNING_MASK: AtomicUsize = AtomicUsize::new(0);
#[cfg(target_os = "none")]
static AP_CPL3_IDLE_MASK: AtomicUsize = AtomicUsize::new(0);
#[cfg(target_os = "none")]
static AP_CPL3_OVERLAP_PASSED: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "none")]
static AP_CPL3_STOP: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "none")]
static PROCESS_PROBE_FAULTS: AtomicUsize = AtomicUsize::new(0);
#[cfg(target_os = "none")]
static PROCESS_PROBE_STARTED: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "none")]
static PROCESS_PROBE_DONE: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "none")]
static LINUX_GETPID_PROBE_PASSED: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "none")]
static LINUX_VFS_DENIAL_PROBE_PASSED: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "none")]
static LINUX_VFS_DENIAL_RESULT: AtomicU64 = AtomicU64::new(0);
#[cfg(target_os = "none")]
static LINUX_CREDENTIAL_PROBE_MASK: AtomicUsize = AtomicUsize::new(0);
#[cfg(target_os = "none")]
static LINUX_MEMORY_PROBE_MASK: AtomicUsize = AtomicUsize::new(0);
#[cfg(target_os = "none")]
static LINUX_MUNMAP_RESULT: AtomicU64 = AtomicU64::new(0);
#[cfg(target_os = "none")]
static AP_PROCESS_SCHEDULING: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "none")]
static PAGE_TABLE_PROBE_PENDING: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "none")]
#[cfg(target_os = "none")]
const PROCESS_VM_PAGE_CAPACITY: usize = 128;
#[cfg(target_os = "none")]
const PROCESS_VM_REGION_CAPACITY: usize = 64;
#[cfg(target_os = "none")]
type RuntimeProcessVm = carlio_os::vma::ProcessVm<
    BootPageTableAccess,
    PROCESS_VM_PAGE_CAPACITY,
    PROCESS_VM_REGION_CAPACITY,
>;
#[cfg(target_os = "none")]
static PROCESS_PROBE_SPACE_A: IrqSpinLock<Option<RuntimeProcessVm>> = IrqSpinLock::new(None);
#[cfg(target_os = "none")]
static PROCESS_PROBE_SPACE_B: IrqSpinLock<Option<RuntimeProcessVm>> = IrqSpinLock::new(None);
#[cfg(target_os = "none")]
static PROCESS_PROBE_SPACE_C: IrqSpinLock<Option<RuntimeProcessVm>> = IrqSpinLock::new(None);
#[cfg(target_os = "none")]
static mut KERNEL_HEAP_BOOT_REGION: KernelHeapBootRegion = KernelHeapBootRegion {
    _bytes: [0; 64 * 1024],
};
#[cfg(target_os = "none")]
static KERNEL_HEAP: IrqSpinLock<KernelHeapSet<256>> = IrqSpinLock::new(KernelHeapSet::new());
#[cfg(target_os = "none")]
#[repr(align(4096))]
struct KernelHeapBootRegion {
    _bytes: [u8; 64 * 1024],
}
#[cfg(target_os = "none")]
#[repr(C, align(16))]
struct ApBootStacks([[u8; AP_STACK_SIZE]; MAX_BOOT_CPUS]);
#[cfg(target_os = "none")]
#[no_mangle]
static mut CARLIO_AP_BOOT_STACKS: ApBootStacks = ApBootStacks([[0; AP_STACK_SIZE]; MAX_BOOT_CPUS]);
#[cfg(target_os = "none")]
static AP_ONLINE_COUNT: AtomicUsize = AtomicUsize::new(1);
#[cfg(target_os = "none")]
static SMP_DETECTED_COUNT: AtomicUsize = AtomicUsize::new(1);
#[cfg(target_os = "none")]
static SMP_BSP_LAPIC_ID: AtomicU32 = AtomicU32::new(u32::MAX);
#[cfg(target_os = "none")]
static FRAMEBUFFER_ACTIVE: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "none")]
const TASK_IDLE: u32 = 0;
#[cfg(target_os = "none")]
const TASK_QUEUED: u32 = 1;
#[cfg(target_os = "none")]
const TASK_RUNNING: u32 = 2;
#[cfg(target_os = "none")]
const TASK_COMPLETE: u32 = 3;
#[cfg(target_os = "none")]
const TASK_SMP_PROBE: u32 = 1;

#[cfg(target_os = "none")]
struct CpuTaskMailbox {
    state: AtomicU32,
    kind: AtomicU32,
    argument: AtomicU64,
    result: AtomicU64,
}
#[cfg(target_os = "none")]
impl CpuTaskMailbox {
    const fn new() -> Self {
        Self {
            state: AtomicU32::new(TASK_IDLE),
            kind: AtomicU32::new(0),
            argument: AtomicU64::new(0),
            result: AtomicU64::new(0),
        }
    }
}

#[cfg(target_os = "none")]
struct BootCpuLocal {
    processor_id: AtomicU32,
    lapic_id: AtomicU32,
    online: AtomicU32,
    task: CpuTaskMailbox,
}
#[cfg(target_os = "none")]
impl BootCpuLocal {
    const fn new() -> Self {
        Self {
            processor_id: AtomicU32::new(u32::MAX),
            lapic_id: AtomicU32::new(u32::MAX),
            online: AtomicU32::new(0),
            task: CpuTaskMailbox::new(),
        }
    }
}
#[cfg(target_os = "none")]
static BOOT_CPU_LOCALS: [BootCpuLocal; MAX_BOOT_CPUS] =
    [const { BootCpuLocal::new() }; MAX_BOOT_CPUS];

#[cfg(target_os = "none")]
#[repr(C)]
struct LimineSmpInfo {
    processor_id: u32,
    lapic_id: u32,
    reserved: u64,
    goto_address: usize,
    extra_argument: u64,
}

#[cfg(target_os = "none")]
#[repr(C)]
struct LimineSmpResponse {
    revision: u64,
    flags: u32,
    bsp_lapic_id: u32,
    cpu_count: u64,
    cpus: *mut *mut LimineSmpInfo,
}

#[cfg(target_os = "none")]
#[repr(C)]
struct LimineSmpRequest {
    id: [u64; 4],
    revision: u64,
    response: *mut LimineSmpResponse,
    flags: u64,
}

#[cfg(target_os = "none")]
#[repr(C)]
struct LimineFramebuffer {
    address: *mut u8,
    width: u64,
    height: u64,
    pitch: u64,
    bits_per_pixel: u16,
    memory_model: u8,
    red_mask_size: u8,
    red_mask_shift: u8,
    green_mask_size: u8,
    green_mask_shift: u8,
    blue_mask_size: u8,
    blue_mask_shift: u8,
    unused: [u8; 7],
    edid_size: u64,
    edid: *mut u8,
    mode_count: u64,
    modes: *mut *mut LimineVideoMode,
}

#[cfg(target_os = "none")]
#[repr(C)]
struct LimineVideoMode {
    pitch: u64,
    width: u64,
    height: u64,
    bits_per_pixel: u16,
    memory_model: u8,
    red_mask_size: u8,
    red_mask_shift: u8,
    green_mask_size: u8,
    green_mask_shift: u8,
    blue_mask_size: u8,
    blue_mask_shift: u8,
}

#[cfg(target_os = "none")]
#[repr(C)]
struct LimineFramebufferResponse {
    revision: u64,
    framebuffer_count: u64,
    framebuffers: *mut *mut LimineFramebuffer,
}

#[cfg(target_os = "none")]
#[repr(C)]
struct LimineFramebufferRequest {
    id: [u64; 4],
    revision: u64,
    response: *mut LimineFramebufferResponse,
}

#[cfg(target_os = "none")]
struct BootPageTableAccess {
    hhdm: u64,
}

#[cfg(target_os = "none")]
impl PageTableAccess for BootPageTableAccess {
    fn read_entry(&self, table: u64, index: usize) -> Result<u64, FrameError> {
        if index >= 512 {
            return Err(FrameError::CorruptTable);
        }
        let address = self
            .hhdm
            .checked_add(table)
            .ok_or(FrameError::InvalidPhysicalAddress)?;
        let slot = (address as *const u64).wrapping_add(index);
        Ok(unsafe { core::ptr::read_volatile(slot) })
    }
    fn write_entry(&mut self, table: u64, index: usize, value: u64) -> Result<(), FrameError> {
        if index >= 512 {
            return Err(FrameError::CorruptTable);
        }
        let address = self
            .hhdm
            .checked_add(table)
            .ok_or(FrameError::InvalidPhysicalAddress)?;
        let slot = (address as *mut u64).wrapping_add(index);
        unsafe { core::ptr::write_volatile(slot, value) };
        Ok(())
    }
    fn allocate_table(&mut self) -> Result<u64, FrameError> {
        let frame = FRAME_ALLOCATOR
            .lock()
            .allocate()
            .ok_or(FrameError::TableAllocationFailed)?;
        let address = match self.hhdm.checked_add(frame) {
            Some(address) => address,
            None => {
                let _ = FRAME_ALLOCATOR.lock().free(frame);
                return Err(FrameError::InvalidPhysicalAddress);
            }
        };
        unsafe { core::ptr::write_bytes(address as *mut u8, 0, 4096) };
        Ok(frame)
    }
    fn release_table(&mut self, physical: u64) -> Result<(), FrameError> {
        FRAME_ALLOCATOR.lock().free(physical)
    }
    fn invalidate_page(&mut self, virtual_address: u64) {
        if lapic::is_initialized() {
            carlio_os::memory::TlbShootdown::invalidate_all(
                &lapic::LapicShootdown,
                0,
                virtual_address,
            );
        } else {
            unsafe {
                asm!("invlpg [{}]", in(reg) virtual_address, options(nostack, preserves_flags))
            };
        }
    }
}

#[cfg(target_os = "none")]
#[cfg(target_os = "none")]
struct ElfAddressSpace<const PAGES: usize>(OwnedAddressSpace<BootPageTableAccess, PAGES>);

#[cfg(target_os = "none")]
impl<const PAGES: usize> ElfAddressSpace<PAGES> {
    fn new(kernel_root: u64) -> Result<Self, FrameError> {
        let hhdm = unsafe { core::ptr::addr_of!(HHDM_OFFSET).read_volatile() };
        OwnedAddressSpace::new(BootPageTableAccess { hhdm }, kernel_root).map(Self)
    }
}

#[cfg(target_os = "none")]
impl<const PAGES: usize> carlio_os::elf::ElfLoadTarget for ElfAddressSpace<PAGES> {
    type Error = FrameError;

    fn map_zeroed_page(&mut self, page: u64) -> Result<(), Self::Error> {
        self.0.map_zeroed(
            page,
            1 | PAGE_FLAG_USER
                | carlio_os::memory::PAGE_FLAG_WRITABLE
                | carlio_os::memory::PAGE_FLAG_NO_EXECUTE,
        )?;
        Ok(())
    }

    fn write(&mut self, mut address: u64, mut bytes: &[u8]) -> Result<(), Self::Error> {
        while !bytes.is_empty() {
            let mapping = match self.0.mapping(address & !0xfff) {
                Ok(mapping) => mapping,
                Err(error) => {
                    write("Runtime: stack/ELF sem mapeamento para VA 0x");
                    write_hex_u64(address);
                    write(".\r\n");
                    return Err(error);
                }
            };
            let page_offset = (address & 0xfff) as usize;
            let count = bytes.len().min(4096 - page_offset);
            let hhdm = unsafe { core::ptr::addr_of!(HHDM_OFFSET).read_volatile() };
            let physical = mapping.physical & !0xfff;
            let destination =
                hhdm.checked_add(physical)
                    .and_then(|base| base.checked_add(page_offset as u64))
                    .ok_or(FrameError::InvalidPhysicalAddress)? as *mut u8;
            unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), destination, count) };
            address = address
                .checked_add(count as u64)
                .ok_or(FrameError::InvalidVirtualAddress)?;
            bytes = &bytes[count..];
        }
        Ok(())
    }

    fn protect_page(&mut self, page: u64, flags: u32) -> Result<(), Self::Error> {
        let mut pte = 1 | PAGE_FLAG_USER;
        if flags & carlio_os::elf::PF_W != 0 {
            pte |= carlio_os::memory::PAGE_FLAG_WRITABLE;
        }
        if flags & carlio_os::elf::PF_X == 0 {
            pte |= carlio_os::memory::PAGE_FLAG_NO_EXECUTE;
        }
        self.0.protect(page, pte)
    }

    fn unmap_page(&mut self, page: u64) -> Result<(), Self::Error> {
        self.0.unmap(page)
    }
}

#[cfg(target_os = "none")]
global_asm!(
    r#"
    .text
    .global carlio_ap_trampoline
carlio_ap_trampoline:
    cli
    movq 24(%rdi), %rax
    cmpq $64, %rax
    jae 2f
    leaq CARLIO_AP_BOOT_STACKS(%rip), %rsp
    shlq $14, %rax
    addq $16384, %rax
    addq %rax, %rsp
    andq $-16, %rsp
    xorq %rbp, %rbp
    call carlio_ap_main
2:
    cli
1:
    pause
    jmp 1b
    "#,
    options(att_syntax)
);

#[cfg(target_os = "none")]
unsafe extern "C" {
    fn carlio_ap_trampoline();
}

#[cfg(target_os = "none")]
#[no_mangle]
extern "C" fn carlio_ap_main(info: *mut LimineSmpInfo) -> ! {
    let index = if !info.is_null() {
        let info = unsafe { &*info };
        let index = info.extra_argument as usize;
        if index < MAX_BOOT_CPUS {
            if !unsafe { idt::load_shared_kernel_gdt(index) } {
                loop {
                    unsafe { asm!("cli; hlt", options(nomem, nostack)) };
                }
            }
            unsafe { idt::load_shared_idt() };
            if unsafe { lapic::initialize(lapic::MMIO_VIRTUAL_BASE) }.is_err() {
                loop {
                    unsafe { asm!("cli; hlt", options(nomem, nostack)) };
                }
            }
            unsafe { lapic::start_scheduler_timer() };
            let local = &BOOT_CPU_LOCALS[index];
            local
                .processor_id
                .store(info.processor_id, Ordering::Relaxed);
            local.lapic_id.store(info.lapic_id, Ordering::Relaxed);
            local.online.store(1, Ordering::Release);
            AP_ONLINE_COUNT.fetch_add(1, Ordering::Release);
            unsafe { asm!("sti", options(nomem, nostack)) };
        }
        index
    } else {
        usize::MAX
    };
    let mut scheduler_timer_started = false;
    loop {
        if !info.is_null() && index < MAX_BOOT_CPUS {
            if ap_acknowledge_process_quiescence(index) {
                continue;
            }
            let mailbox = &BOOT_CPU_LOCALS[index].task;
            if mailbox
                .state
                .compare_exchange(
                    TASK_QUEUED,
                    TASK_RUNNING,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                let value = match mailbox.kind.load(Ordering::Relaxed) {
                    TASK_SMP_PROBE => smp_probe_work(mailbox.argument.load(Ordering::Relaxed)),
                    _ => 0,
                };
                mailbox.result.store(value, Ordering::Relaxed);
                mailbox.state.store(TASK_COMPLETE, Ordering::Release);
                continue;
            }
            if AP_PROCESS_SCHEDULING.load(Ordering::Acquire) {
                if PROCESS_PROBE_STARTED.load(Ordering::Acquire)
                    && !scheduler_has_current_process(index)
                {
                    AP_CPL3_IDLE_MASK.fetch_or(1usize << index, Ordering::AcqRel);
                }
                if !scheduler_timer_started {
                    unsafe { lapic::start_scheduler_timer() };
                    scheduler_timer_started = true;
                }
                if let Some(process) = PROCESS_SCHEDULER.schedule_local(index).ok().flatten() {
                    if process.interrupt_frame != 0
                        && process.kernel_stack_top != 0
                        && unsafe {
                            idt::activate_cpu_process_address_space(
                                index,
                                process.page_table,
                                process.kernel_stack_top,
                            )
                        }
                    {
                        if is_secondary_cpu(index) && is_ap_cpl3_probe_id(process.id) {
                            mark_ap_cpl3_running(index);
                        }
                        unsafe {
                            idt::enter_user_frame(
                                process.interrupt_frame as *mut idt::UserInterruptFrame,
                            )
                        };
                    }
                }
            }
        }
        core::hint::spin_loop();
    }
}

#[cfg(target_os = "none")]
#[no_mangle]
extern "C" fn carlio_ap_idle_resume() -> ! {
    let cpu = current_scheduler_cpu();
    let stack_top = unsafe {
        core::ptr::addr_of_mut!(CARLIO_AP_BOOT_STACKS.0[cpu])
            .cast::<u8>()
            .add(AP_STACK_SIZE) as u64
    };
    unsafe {
        asm!(
            "mov rsp, {stack_top}",
            "and rsp, -16",
            "mov edi, {cpu:e}",
            "call carlio_ap_scheduler_idle",
            stack_top = in(reg) stack_top,
            cpu = in(reg) cpu,
            options(noreturn)
        );
    }
}

#[cfg(target_os = "none")]
#[no_mangle]
extern "C" fn carlio_ap_scheduler_idle(cpu: usize) -> ! {
    let stack_top = unsafe {
        core::ptr::addr_of_mut!(CARLIO_AP_BOOT_STACKS.0[cpu])
            .cast::<u8>()
            .add(AP_STACK_SIZE) as u64
    };
    let kernel_root = PROCESS_KERNEL_CR3.load(Ordering::Acquire);
    if kernel_root != 0
        && unsafe { idt::activate_cpu_process_address_space(cpu, kernel_root, stack_top) }
        && PROCESS_PROBE_STARTED.load(Ordering::Acquire)
    {
        AP_CPL3_IDLE_MASK.fetch_or(1usize << cpu, Ordering::AcqRel);
    }
    loop {
        if ap_acknowledge_process_quiescence(cpu) {
            continue;
        }
        if let Some(process) = PROCESS_SCHEDULER.schedule_local(cpu).ok().flatten() {
            if process.interrupt_frame != 0
                && process.kernel_stack_top != 0
                && unsafe {
                    idt::activate_cpu_process_address_space(
                        cpu,
                        process.page_table,
                        process.kernel_stack_top,
                    )
                }
            {
                if is_secondary_cpu(cpu) && is_ap_cpl3_probe_id(process.id) {
                    mark_ap_cpl3_running(cpu);
                }
                unsafe {
                    idt::enter_user_frame(process.interrupt_frame as *mut idt::UserInterruptFrame)
                };
            }
        }
        core::hint::spin_loop();
    }
}

#[cfg(target_os = "none")]
fn ap_acknowledge_process_quiescence(cpu: usize) -> bool {
    let Some(epoch) = PROCESS_REAP_BARRIER.requested() else {
        return false;
    };
    if cpu >= MAX_BOOT_CPUS {
        return false;
    }
    let stack_top = unsafe {
        core::ptr::addr_of_mut!(CARLIO_AP_BOOT_STACKS.0[cpu])
            .cast::<u8>()
            .add(AP_STACK_SIZE) as u64
    };
    let kernel_root = PROCESS_KERNEL_CR3.load(Ordering::Acquire);
    if kernel_root == 0
        || !unsafe { idt::activate_cpu_process_address_space(cpu, kernel_root, stack_top) }
    {
        loop {
            unsafe { asm!("cli; hlt", options(nomem, nostack)) };
        }
    }
    let current_root: u64;
    // SAFETY: CR3 é lido em long mode para validar que esta CPU já está na raiz do kernel.
    unsafe { asm!("mov {}, cr3", out(reg) current_root, options(nomem, nostack, preserves_flags)) };
    let bit = 1usize << cpu;
    if current_root & 0x000f_ffff_ffff_f000 != kernel_root
        || AP_CPL3_RUNNING_MASK.load(Ordering::Acquire) & bit != 0
    {
        return false;
    }
    AP_CPL3_IDLE_MASK.fetch_or(1usize << cpu, Ordering::AcqRel);
    if !PROCESS_REAP_BARRIER.acknowledge(cpu, epoch) {
        return false;
    }
    while PROCESS_REAP_BARRIER.requested() == Some(epoch) {
        core::hint::spin_loop();
    }
    true
}

#[cfg(target_os = "none")]
fn smp_probe_work(seed: u64) -> u64 {
    let mut value = seed;
    for step in 0..32_768u64 {
        value = value.rotate_left(7) ^ step.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
    value
}

#[repr(C)]
struct LimineKernelAddressResponse {
    revision: u64,
    physical_base: u64,
    virtual_base: u64,
}

#[repr(C)]
struct LimineKernelAddressRequest {
    id: [u64; 4],
    revision: u64,
    response: *mut LimineKernelAddressResponse,
}

#[repr(C)]
struct LimineHhdmResponse {
    revision: u64,
    offset: u64,
}

#[repr(C)]
struct LimineHhdmRequest {
    id: [u64; 4],
    revision: u64,
    response: *mut LimineHhdmResponse,
}

#[repr(C)]
struct LimineMemmapEntry {
    base: u64,
    length: u64,
    kind: u64,
}

#[repr(C)]
struct LimineMemmapResponse {
    revision: u64,
    entry_count: u64,
    entries: *mut *mut LimineMemmapEntry,
}

#[repr(C)]
struct LimineMemmapRequest {
    id: [u64; 4],
    revision: u64,
    response: *mut LimineMemmapResponse,
}

#[used]
#[link_section = ".limine_requests"]
static mut HHDM_REQUEST: LimineHhdmRequest = LimineHhdmRequest {
    id: [
        0xc7b1_dd30_df4c_8b88,
        0x0a82_e883_a194_f07b,
        0x48dc_f1cb_8ad2_b852,
        0x6398_4e95_9a98_244b,
    ],
    revision: 0,
    response: core::ptr::null_mut(),
};

#[used]
#[link_section = ".limine_requests"]
static mut MEMMAP_REQUEST: LimineMemmapRequest = LimineMemmapRequest {
    id: [
        0xc7b1_dd30_df4c_8b88,
        0x0a82_e883_a194_f07b,
        0x67cf_3d9d_378a_806f,
        0xe304_acdf_c50c_3c62,
    ],
    revision: 0,
    response: core::ptr::null_mut(),
};

#[used]
#[link_section = ".limine_requests"]
static mut KERNEL_ADDRESS_REQUEST: LimineKernelAddressRequest = LimineKernelAddressRequest {
    id: [
        0xc7b1_dd30_df4c_8b88,
        0x0a82_e883_a194_f07b,
        0x71ba_7686_3cc5_5f63,
        0xb264_4a48_c516_a487,
    ],
    revision: 0,
    response: core::ptr::null_mut(),
};

#[cfg(target_os = "none")]
#[used]
#[link_section = ".limine_requests"]
static mut SMP_REQUEST: LimineSmpRequest = LimineSmpRequest {
    id: [
        0xc7b1_dd30_df4c_8b88,
        0x0a82_e883_a194_f07b,
        0x95a6_7b81_9a1b_857e,
        0xa0b6_1b72_3b6a_73e0,
    ],
    revision: 0,
    response: core::ptr::null_mut(),
    flags: 0,
};

#[cfg(target_os = "none")]
#[used]
#[link_section = ".limine_requests"]
static mut FRAMEBUFFER_REQUEST: LimineFramebufferRequest = LimineFramebufferRequest {
    id: [
        0xc7b1_dd30_df4c_8b88,
        0x0a82_e883_a194_f07b,
        0x9d58_27dc_d881_dd75,
        0xa314_8604_f6fa_b11b,
    ],
    revision: 1,
    response: core::ptr::null_mut(),
};

#[cfg(target_os = "none")]
#[path = "arch/x86_64/ahci.rs"]
mod ahci;
#[cfg(target_os = "none")]
#[path = "arch/x86_64/e1000.rs"]
mod e1000;
#[cfg(target_os = "none")]
#[path = "arch/x86_64/idt.rs"]
mod idt;
#[cfg(target_os = "none")]
#[path = "arch/x86_64/lapic.rs"]
mod lapic;
#[cfg(target_os = "none")]
#[path = "arch/x86_64/pci.rs"]
mod pci_device;

#[used]
#[link_section = ".limine_requests_start"]
static REQUESTS_START: [u64; 2] = [0xf6b8_f4b3_9de7_d1ae, 0xfab9_1a69_40fc_b9cf];

#[used]
#[link_section = ".limine_requests"]
static BASE_REVISION: [u64; 3] = [0xf956_2b2d_5c95_a6c8, 0x6a7b_3849_4453_6bdc, 3];

#[used]
#[link_section = ".limine_requests_end"]
static REQUESTS_END: [u64; 2] = [0xadc0_e053_1bb1_0d03, 0x9572_709f_3176_4c62];

#[no_mangle]
#[cfg(target_os = "none")]
pub extern "C" fn _start() -> ! {
    serial_init();
    unsafe { idt::install() };
    startup_screen();
    video_boot_probe();
    write("IDT: exceções 0-31 registradas; IRQs externos permanecem desativados.\r\n");
    if idt::ring3_descriptors_ready() {
        write("Ring 3: GDT, TSS, RSP0 e seletores de usuário configurados; CPL continua 0.\r\n");
    } else {
        write("Ring 3: configuração GDT/TSS inválida.\r\n");
        record_error(KernelError::DeviceInitFailed);
    }
    write("Aviso: terminal de diagnóstico em ring 0; não é uma fronteira de segurança.\r\n");
    report_memory_map();
    let hhdm = unsafe { core::ptr::addr_of!(HHDM_OFFSET).read_volatile() };
    if prepare_lapic_mmio(hhdm) && unsafe { lapic::initialize(lapic::MMIO_VIRTUAL_BASE) }.is_ok() {
        write("LAPIC xAPIC ativo no BSP; vetor de shootdown remoto 0xF1 instalado.\r\n");
        start_secondary_processors();
    } else {
        write("LAPIC indisponível; SMP e shootdown remoto desativados por segurança.\r\n");
    }
    lapic::set_online_aps(AP_ONLINE_COUNT.load(Ordering::Acquire).saturating_sub(1));
    run_smp_parallel_probe();
    kernel_heap_boot_probe();
    enumerate_pci();
    initialize_persistent_rootfs();
    unsafe { idt::install_timer_frame_switch(timer_process_switch) };
    if PAGE_TABLE_PROBE_PENDING.swap(false, Ordering::AcqRel) {
        let hhdm = unsafe { core::ptr::addr_of!(HHDM_OFFSET).read_volatile() };
        page_table_boot_probe(hhdm);
    }
    // O probe CPL3 pode retornar ao kernel com IF desabilitado. Reativar e
    // reinicializar o PIT após a prova para que o watchdog também cubra o shell.
    unsafe { idt::start_watchdog() };
    if lapic::is_initialized() {
        unsafe { lapic::start_scheduler_timer() };
    }
    AP_PROCESS_SCHEDULING.store(true, Ordering::Release);
    write("Watchdog PIT ativo: prazo de 5 s sem progresso.\r\n");
    write("Digite 'ajuda' para ver os comandos.\r\n\r\n");
    shell()
}

#[cfg(target_os = "none")]
fn start_secondary_processors() {
    let response = unsafe { core::ptr::addr_of!(SMP_REQUEST.response).read_volatile() };
    if response.is_null() {
        write("SMP: resposta do Limine ausente; somente BSP ativo.\r\n");
        return;
    }
    let cpu_count = unsafe { (*response).cpu_count };
    let cpus = unsafe { (*response).cpus };
    let bsp_lapic_id = unsafe { (*response).bsp_lapic_id };
    if cpus.is_null() || cpu_count == 0 {
        write("SMP: inventário de CPUs inválido; somente BSP ativo.\r\n");
        record_error(KernelError::BootInfoMissing);
        return;
    }

    let limit = cpu_count.min(MAX_BOOT_CPUS as u64) as usize;
    SMP_DETECTED_COUNT.store(limit, Ordering::Release);
    SMP_BSP_LAPIC_ID.store(bsp_lapic_id, Ordering::Release);
    let trampoline = carlio_ap_trampoline as *const () as usize;
    let mut requested = 0usize;
    for index in 0..limit {
        let info = unsafe { cpus.add(index).read_volatile() };
        if info.is_null() {
            continue;
        }
        let processor_id = unsafe { (*info).processor_id };
        let lapic_id = unsafe { (*info).lapic_id };
        if lapic_id == bsp_lapic_id {
            let local = &BOOT_CPU_LOCALS[index];
            local.processor_id.store(processor_id, Ordering::Relaxed);
            local.lapic_id.store(lapic_id, Ordering::Relaxed);
            local.online.store(1, Ordering::Release);
            let _ = unsafe { idt::load_shared_kernel_gdt(index) };
            continue;
        }
        unsafe {
            core::ptr::addr_of_mut!((*info).extra_argument).write(index as u64);
            AtomicUsize::from_ptr(core::ptr::addr_of_mut!((*info).goto_address))
                .store(trampoline, Ordering::Release);
        }
        requested += 1;
    }

    let mut spins = 0usize;
    while AP_ONLINE_COUNT.load(Ordering::Acquire) < requested + 1 && spins < 10_000_000 {
        core::hint::spin_loop();
        spins += 1;
    }
    let online = AP_ONLINE_COUNT.load(Ordering::Acquire);
    write("SMP Limine: CPUs detectadas=");
    write_u32(cpu_count.min(u32::MAX as u64) as u32);
    write(", APs iniciadas=");
    write_u32(online.saturating_sub(1).min(u32::MAX as usize) as u32);
    write(", aguardadas=");
    write_u32(requested.min(u32::MAX as usize) as u32);
    write("; APs aguardam despacho de tarefa, scheduler ainda desativado.\r\n");
}

/// Mapeia a página física local APIC como uncached antes de habilitar xAPIC.
#[cfg(target_os = "none")]
fn prepare_lapic_mmio(hhdm: u64) -> bool {
    let physical = match lapic::physical_base() {
        Ok(address) if address != 0 => address,
        _ => return false,
    };
    let root: u64;
    unsafe { asm!("mov {}, cr3", out(reg) root, options(nomem, nostack, preserves_flags)) };
    let root = root & 0x000f_ffff_ffff_f000;
    let mut tables = match PageTableManager::new(BootPageTableAccess { hhdm }, root) {
        Ok(manager) => manager,
        Err(_) => return false,
    };
    let mapped = match tables.mapping(lapic::MMIO_VIRTUAL_BASE) {
        Ok(mapping) => mapping.physical == physical && mapping.flags & 0x18 == 0x18,
        Err(FrameError::NotMapped) => tables.map(lapic::MMIO_VIRTUAL_BASE, physical, 0x1b).is_ok(), // P | RW | PWT | PCD: MMIO sem cache
        Err(_) => false,
    };
    mapped && lapic::set_mmio_physical_base(physical)
}

#[cfg(target_os = "none")]
fn run_smp_parallel_probe() {
    let cpu_count = SMP_DETECTED_COUNT.load(Ordering::Acquire);
    let bsp_lapic = SMP_BSP_LAPIC_ID.load(Ordering::Acquire);
    let mut targets = [usize::MAX; MAX_BOOT_CPUS];
    let mut target_count = 0usize;
    for index in 0..cpu_count {
        let local = &BOOT_CPU_LOCALS[index];
        if local.online.load(Ordering::Acquire) == 0
            || local.lapic_id.load(Ordering::Relaxed) == bsp_lapic
        {
            continue;
        }
        let seed = 0x4341_524c_494f_0000u64 | index as u64;
        local.task.argument.store(seed, Ordering::Relaxed);
        local.task.kind.store(TASK_SMP_PROBE, Ordering::Relaxed);
        local.task.state.store(TASK_QUEUED, Ordering::Release);
        targets[target_count] = index;
        target_count += 1;
    }

    let mut spins = 0usize;
    loop {
        let complete = targets[..target_count].iter().all(|&index| {
            BOOT_CPU_LOCALS[index].task.state.load(Ordering::Acquire) == TASK_COMPLETE
        });
        if complete || spins >= 10_000_000 {
            break;
        }
        spins += 1;
        core::hint::spin_loop();
    }

    let mut completed = 0usize;
    let mut valid = 0usize;
    for &index in &targets[..target_count] {
        let mailbox = &BOOT_CPU_LOCALS[index].task;
        if mailbox.state.load(Ordering::Acquire) == TASK_COMPLETE {
            completed += 1;
            let seed = 0x4341_524c_494f_0000u64 | index as u64;
            if mailbox.result.load(Ordering::Relaxed) == smp_probe_work(seed) {
                valid += 1;
            }
            mailbox.state.store(TASK_IDLE, Ordering::Release);
        }
    }
    write("SMP trabalho: CPUs AP=");
    write_u32(target_count.min(u32::MAX as usize) as u32);
    write(", tarefas concluídas=");
    write_u32(completed.min(u32::MAX as usize) as u32);
    write(", resultados válidos=");
    write_u32(valid.min(u32::MAX as usize) as u32);
    write("; despacho de prova concluído.\r\n");
    if completed != target_count || valid != target_count {
        record_error(KernelError::DeviceInitFailed);
    }
}

#[cfg(not(target_os = "none"))]
fn main() {}

#[cfg(target_os = "none")]
fn shell() -> ! {
    let mut line = [0u8; INPUT_LIMIT];
    loop {
        write("carlio> ");
        let mut len = 0;
        let mut overflowed = false;
        loop {
            let byte = match serial_read() {
                Ok(byte) => byte,
                Err(_) => continue,
            };
            match byte {
                b'\r' | b'\n' => {
                    write("\r\n");
                    break;
                }
                8 | 127 if len > 0 => {
                    len -= 1;
                    write("\x08 \x08");
                }
                32..=126 => {
                    if len < INPUT_LIMIT {
                        line[len] = byte;
                        len += 1;
                        serial_write(byte);
                    } else if !overflowed {
                        overflowed = true;
                        record_error(KernelError::InputTooLong);
                    }
                }
                _ => {}
            }
        }
        if overflowed {
            write("Entrada descartada por exceder o limite.\r\n");
            continue;
        }
        let command = core::str::from_utf8(&line[..len]).unwrap_or("");
        #[cfg(all(target_os = "none", feature = "storage-test"))]
        if command == "diskteste" {
            write("Teste AHCI em disco QEMU descartável...\r\n");
            match unsafe { ahci::disposable_disk_test() } {
                Ok(()) => {
                    write("AHCI: IDENTIFY, WRITE, READ e erro do controlador verificados.\r\n")
                }
                Err(error) => {
                    record_error(error);
                    write("AHCI: teste falhou: ");
                    write(error.message());
                    write("\r\n");
                }
            }
            continue;
        }
        #[cfg(all(target_os = "none", feature = "storage-test"))]
        if command == "fs-teste" {
            write("Formatando volume Hermione no disco QEMU descartavel...\r\n");
            match unsafe { ahci::disposable_filesystem_test() } {
                Ok(()) => write(
                    "Hermione: volume persistente, espelhamento, journal/commit, remontagem e scrub passaram.\r\n",
                ),
                Err(error) => {
                    record_error(error);
                    write("Hermione: teste falhou: ");
                    write(error.message());
                    write("\r\n");
                }
            }
            continue;
        }
        #[cfg(feature = "crash-test")]
        match command {
            "panic-teste" => panic!("panic intencional para teste do capturador"),
            "fault-teste" => unsafe { asm!("ud2", options(nomem, nostack)) },
            "hang-teste" => loop {
                core::hint::spin_loop()
            },
            "doublefault-teste" => unsafe {
                asm!("mov rsp, 0", "ud2", options(nomem, nostack, noreturn))
            },
            _ => {}
        }
        match parse_command(command) {
            Command::Help => write("Comandos: ajuda, sobre, usuario, permissoes, erros, logs, limpar, parar.\r\n"),
            Command::About => {
                write("carlIO OS | Criado por João G.R.Santos\r\n");
                write("Sistema operacional experimental x86_64 escrito em Rust.\r\n");
            }
            Command::User => write("usuário: núcleo do sistema (nível de privilégio 0)\r\n"),
            Command::Permissions => write("Política demonstrativa: convidado pode consultar o estado; alteração do sistema é negada.\r\nAviso: os processos ainda não estão isolados; o núcleo inteiro executa no nível 0.\r\n"),
            Command::Errors => report_errors(),
            #[cfg(target_os = "none")]
            Command::Logs => report_errors_markdown(),
            #[cfg(not(target_os = "none"))]
            Command::Logs => {},
            Command::Clear => write("\x1b[2J\x1b[H"),
            Command::Halt => {
                write("Processador parado.\r\n");
                loop { unsafe { asm!("cli; hlt", options(nomem, nostack)); } }
            }
            Command::Unknown => write("Comando desconhecido. Digite 'ajuda'.\r\n"),
        }
    }
}

#[cfg(target_os = "none")]
fn startup_screen() {
    write("\x1b[2J\x1b[H");
    write("+----------------------------------------------------+\r\n");
    write("| carlIO OS | Criado por João G.R.Santos             |\r\n");
    write("| Sistema operacional experimental x86_64            |\r\n");
    write("+----------------------------------------------------+\r\n");
    write("Núcleo Rust · inicialização serial\r\n\r\n");
}

#[cfg(target_os = "none")]
fn serial_init() {
    unsafe {
        out(COM1 + 1, 0x00);
        out(COM1 + 3, 0x80);
        out(COM1, 0x03);
        out(COM1 + 1, 0x00);
        out(COM1 + 3, 0x03);
        out(COM1 + 2, 0xc7);
        out(COM1 + 4, 0x0b);
    }
}

#[cfg(target_os = "none")]
fn serial_write(byte: u8) -> bool {
    for iteration in 0..SERIAL_POLL_LIMIT {
        if iteration & 0x0fff == 0 {
            #[cfg(target_os = "none")]
            idt::heartbeat();
        }
        if unsafe { input(COM1 + 5) } & 0x20 != 0 {
            unsafe {
                out(COM1, byte);
            }
            return true;
        }
        core::hint::spin_loop();
    }
    record_error(KernelError::SerialWriteTimeout);
    false
}

#[cfg(target_os = "none")]
fn serial_read() -> Result<u8, KernelError> {
    // APs poll independently so an idle reader cannot monopolize the UART.
    // Ownership covers only the final status recheck and RBR consume, keeping
    // two CPUs from racing on the single receive register.
    for iteration in 0..SERIAL_READ_POLL_LIMIT {
        // SAFETY: COM1 was initialized during boot; this bounded poll only
        // reads the UART line-status register.
        if unsafe { input(COM1 + 5) } & 1 != 0 {
            let Some(_claim) = SERIAL_RX_OWNER.try_claim() else {
                core::hint::spin_loop();
                continue;
            };
            // SAFETY: SERIAL_RX_OWNER grants one CPU exclusive access to the
            // status recheck and receive-buffer register consumption.
            let byte = if unsafe { input(COM1 + 5) } & 1 != 0 {
                Some(unsafe { input(COM1) })
            } else {
                None
            };
            if let Some(byte) = byte {
                return Ok(byte);
            }
        }
        if iteration & 0x0fff == 0 {
            idt::heartbeat();
        }
        core::hint::spin_loop();
    }
    record_error(KernelError::SerialReadTimeout);
    Err(KernelError::SerialReadTimeout)
}

#[cfg(target_os = "none")]
fn write(text: &str) {
    for byte in text.bytes() {
        if !serial_write(byte) {
            break;
        }
    }
}

#[cfg(target_os = "none")]
fn video_boot_probe() {
    use carlio_os::video::{Framebuffer, PixelFormat};

    const MAX_FRAMEBUFFER_BYTES: usize = 128 * 1024 * 1024;
    let response = unsafe { core::ptr::addr_of!(FRAMEBUFFER_REQUEST.response).read_volatile() };
    if response.is_null() {
        write("Vídeo: framebuffer indisponível; kernel continua em modo serial.\r\n");
        return;
    }
    let response = unsafe { &*response };
    if response.framebuffer_count == 0
        || response.framebuffer_count > 8
        || response.framebuffers.is_null()
    {
        write("Vídeo: resposta de framebuffer inválida; modo serial mantido.\r\n");
        return;
    }
    let framebuffer_pointer = unsafe { response.framebuffers.read() };
    if framebuffer_pointer.is_null() {
        write("Vídeo: framebuffer nulo; modo serial mantido.\r\n");
        return;
    }
    let info = unsafe { &*framebuffer_pointer };
    let width = match usize::try_from(info.width) {
        Ok(value) if value <= 8192 => value,
        _ => {
            write("Vídeo: largura de framebuffer inválida.\r\n");
            return;
        }
    };
    let height = match usize::try_from(info.height) {
        Ok(value) if value <= 8192 => value,
        _ => {
            write("Vídeo: altura de framebuffer inválida.\r\n");
            return;
        }
    };
    let pitch = match usize::try_from(info.pitch) {
        Ok(value) => value,
        _ => {
            write("Vídeo: pitch do framebuffer inválido.\r\n");
            return;
        }
    };
    let length = match pitch.checked_mul(height) {
        Some(value) if value <= MAX_FRAMEBUFFER_BYTES => value,
        _ => {
            write("Vídeo: tamanho do framebuffer fora do limite.\r\n");
            return;
        }
    };
    if info.address.is_null()
        || (info.address as usize).checked_add(length).is_none()
        || info.memory_model != 1
    {
        write("Vídeo: endereço ou modelo de pixel não suportado.\r\n");
        return;
    }
    let format = PixelFormat {
        bits_per_pixel: info.bits_per_pixel as u8,
        red_size: info.red_mask_size,
        red_shift: info.red_mask_shift,
        green_size: info.green_mask_size,
        green_shift: info.green_mask_shift,
        blue_size: info.blue_mask_size,
        blue_shift: info.blue_mask_shift,
    };
    let bytes = unsafe { core::slice::from_raw_parts_mut(info.address, length) };
    let mut framebuffer = match Framebuffer::new(bytes, width, height, pitch, format) {
        Ok(fb) => fb,
        Err(_) => {
            write("Vídeo: formato de framebuffer recusado; modo serial mantido.\r\n");
            return;
        }
    };
    let stripe_width = width.min(320) / 4;
    framebuffer.fill_rect(0, 0, width.min(320), height.min(32), [20, 28, 45]);
    framebuffer.fill_rect(8, 8, stripe_width, 12, [230, 75, 75]);
    framebuffer.fill_rect(8 + stripe_width, 8, stripe_width, 12, [80, 200, 120]);
    framebuffer.fill_rect(8 + stripe_width * 2, 8, stripe_width, 12, [80, 140, 240]);
    FRAMEBUFFER_ACTIVE.store(true, Ordering::Release);
    write("Framebuffer Limine: RGB ativo; renderização de pixels validada.\r\n");
}

#[cfg(target_os = "none")]
extern "C" fn timer_process_switch(
    cpu: usize,
    frame: *mut idt::UserInterruptFrame,
) -> *mut idt::UserInterruptFrame {
    if frame.is_null() {
        return frame;
    }
    let previous = PROCESS_SCHEDULER.current(cpu);
    let previous_is_ap_probe =
        is_secondary_cpu(cpu) && previous.is_some_and(|process| is_ap_cpl3_probe_id(process.id));
    if previous_is_ap_probe && frame_user_mode(frame) {
        let bit = 1usize << cpu;
        let active = AP_CPL3_RUNNING_MASK.fetch_and(!bit, Ordering::AcqRel);
        let other_aps = active & secondary_cpu_mask() & !bit;
        if active & bit != 0 && other_aps != 0 {
            AP_CPL3_CPU_MASK.fetch_or(bit | other_aps, Ordering::AcqRel);
            AP_CPL3_OVERLAP_PASSED.store(true, Ordering::Release);
            AP_CPL3_STOP.store(true, Ordering::Release);
        }
    }
    if AP_CPL3_STOP.load(Ordering::Acquire) && previous_is_ap_probe && frame_user_mode(frame) {
        return frame;
    }
    if PROCESS_PROBE_STARTED.load(Ordering::Acquire) && previous.is_some() && frame_user_mode(frame)
    {
        PROCESS_PROBE_SWITCHES.fetch_add(1, Ordering::Relaxed);
    }
    let next = PROCESS_SCHEDULER.preempt_frame_local(cpu, frame as u64);
    let Ok(Some(process)) = next else {
        if previous_is_ap_probe && frame_user_mode(frame) {
            mark_ap_cpl3_running(cpu);
        }
        return frame;
    };
    if PROCESS_PROBE_STARTED.load(Ordering::Acquire)
        && previous.is_some_and(|old| old.id != process.id)
    {
        if PROCESS_PROBE_SWITCHES.load(Ordering::Relaxed) == 1 {
            write("Timer: primeira preempção CPL3 observada.\r\n");
        }
        if previous.is_some_and(|old| old.page_table != process.page_table) {
            PROCESS_PROBE_CR3_SWITCHES.fetch_add(1, Ordering::Relaxed);
        }
        idt::heartbeat();
    }
    if process.interrupt_frame == 0 || process.kernel_stack_top == 0 {
        return frame;
    }
    if !unsafe {
        idt::activate_cpu_process_address_space(cpu, process.page_table, process.kernel_stack_top)
    } {
        loop {
            unsafe { asm!("cli; hlt", options(nomem, nostack)) };
        }
    }
    if is_secondary_cpu(cpu) && is_ap_cpl3_probe_id(process.id) {
        mark_ap_cpl3_running(cpu);
    }
    process.interrupt_frame as *mut idt::UserInterruptFrame
}

#[cfg(target_os = "none")]
fn is_ap_cpl3_probe_id(id: u64) -> bool {
    id != 0
        && (id == PROCESS_PROBE_IDS[1].load(Ordering::Acquire)
            || id == PROCESS_PROBE_IDS[2].load(Ordering::Acquire))
}

#[cfg(target_os = "none")]
fn secondary_cpu_mask() -> usize {
    let mut mask = 0usize;
    for cpu in 0..SMP_DETECTED_COUNT
        .load(Ordering::Acquire)
        .min(MAX_BOOT_CPUS)
    {
        if BOOT_CPU_LOCALS[cpu].online.load(Ordering::Acquire) != 0 && is_secondary_cpu(cpu) {
            mask |= 1usize << cpu;
        }
    }
    mask
}

#[cfg(target_os = "none")]
fn mark_ap_cpl3_running(cpu: usize) {
    if cpu < MAX_BOOT_CPUS {
        let bit = 1usize << cpu;
        AP_CPL3_IDLE_MASK.fetch_and(!bit, Ordering::AcqRel);
        AP_CPL3_RUNNING_MASK.fetch_or(bit, Ordering::Release);
    }
}

#[cfg(target_os = "none")]
pub(crate) fn stop_ap_process_probe(cpu: usize) -> bool {
    if !is_secondary_cpu(cpu) || !AP_CPL3_STOP.load(Ordering::Acquire) {
        return false;
    }
    if PROCESS_SCHEDULER.current(cpu).is_none() {
        return true;
    }
    PROCESS_SCHEDULER.exit(cpu, 0).is_ok()
}

#[cfg(target_os = "none")]
fn frame_user_mode(frame: *mut idt::UserInterruptFrame) -> bool {
    // The interrupt frame is supplied by the x86 entry stub for this CPU.
    unsafe { !frame.is_null() && (*frame).cs & 3 == 3 }
}

#[cfg(target_os = "none")]
pub(crate) fn begin_process_switch_probe() -> Option<*mut idt::UserInterruptFrame> {
    const CODE_VA: u64 = 0x0000_4000_0000_3000;
    const STACK_VA: u64 = CODE_VA + 4096;
    if PROCESS_PROBE_STARTED.swap(true, Ordering::AcqRel) {
        return None;
    }
    write("Runtime de processos: preparando tarefas CPL3.\r\n");
    PROCESS_PROBE_SWITCHES.store(0, Ordering::Relaxed);
    PROCESS_PROBE_CR3_SWITCHES.store(0, Ordering::Relaxed);
    PROCESS_PROBE_FAULTS.store(0, Ordering::Relaxed);
    LINUX_GETPID_PROBE_PASSED.store(false, Ordering::Relaxed);
    LINUX_VFS_DENIAL_PROBE_PASSED.store(false, Ordering::Relaxed);
    LINUX_VFS_DENIAL_RESULT.store(0, Ordering::Relaxed);
    LINUX_CREDENTIAL_PROBE_MASK.store(0, Ordering::Relaxed);
    LINUX_MEMORY_PROBE_MASK.store(0, Ordering::Relaxed);
    LINUX_MUNMAP_RESULT.store(0, Ordering::Relaxed);
    AP_CPL3_CPU_MASK.store(0, Ordering::Relaxed);
    AP_CPL3_RUNNING_MASK.store(0, Ordering::Relaxed);
    AP_CPL3_IDLE_MASK.store(0, Ordering::Relaxed);
    AP_CPL3_OVERLAP_PASSED.store(false, Ordering::Relaxed);
    AP_CPL3_STOP.store(false, Ordering::Relaxed);
    let mut root: u64;
    unsafe { asm!("mov {}, cr3", out(reg) root, options(nomem, nostack, preserves_flags)) };
    root &= 0x000f_ffff_ffff_f000;
    // CPL3 exercises write, identity getters, getgroups, getpid and exit.
    // The write buffer shares the read-only executable page, so the kernel
    // must validate/read it without requiring user write permission.
    let (task, task_len) = linux_syscall_probe_task();
    let mut fault_task = [0u8; 13];
    fault_task[..2].copy_from_slice(&[0xeb, 0xfe]); // AP fixture stays in CPL3 until the overlap witness.
    let image_a = probe_elf_image(&task[..task_len]);
    let image_b = probe_elf_image(&fault_task);
    if !install_process_probe_images(&image_a, &image_b) {
        write("Runtime: não foi possível instalar as imagens no VFS.");
        PROCESS_PROBE_STARTED.store(false, Ordering::Release);
        return None;
    }
    let Some((space_a, entry_a, user_stack_a)) = make_process_probe_space(
        root,
        STACK_VA,
        "/programas/probe",
        &["probe", "--spin"],
        &["PATH=/programas", "CARLIO_PROBE=1"],
    ) else {
        write("Runtime: falha ao criar espaço virtual A.\r\n");
        PROCESS_PROBE_STARTED.store(false, Ordering::Release);
        return None;
    };
    let Some((space_b, entry_b, user_stack_b)) = make_process_probe_space(
        root,
        STACK_VA,
        "/programas/fault",
        &["fault-probe"],
        &["CARLIO_PROBE=1"],
    ) else {
        write("Runtime: falha ao criar espaço virtual B.\r\n");
        let _ = space_a.try_destroy();
        PROCESS_PROBE_STARTED.store(false, Ordering::Release);
        return None;
    };
    let Some((space_c, entry_c, user_stack_c)) = make_process_probe_space(
        root,
        STACK_VA,
        "/programas/fault",
        &["ap-probe-2"],
        &["CARLIO_PROBE=1"],
    ) else {
        write("Runtime: falha ao criar espaço virtual C.\r\n");
        let _ = space_a.try_destroy();
        let _ = space_b.try_destroy();
        PROCESS_PROBE_STARTED.store(false, Ordering::Release);
        return None;
    };
    let root_a = space_a.root();
    let root_b = space_b.root();
    let root_c = space_c.root();
    let mut vm_a = RuntimeProcessVm::new(
        space_a,
        carlio_os::vma::USER_HEAP_BASE,
        carlio_os::vma::USER_HEAP_BASE + 0x1000_0000,
    );
    let mut vm_b = RuntimeProcessVm::new(
        space_b,
        carlio_os::vma::USER_HEAP_BASE,
        carlio_os::vma::USER_HEAP_BASE + 0x1000_0000,
    );
    let mut vm_c = RuntimeProcessVm::new(
        space_c,
        carlio_os::vma::USER_HEAP_BASE,
        carlio_os::vma::USER_HEAP_BASE + 0x1000_0000,
    );
    if vm_a.reserve_existing(CODE_VA, 2 * 4096).is_err()
        || vm_b.reserve_existing(CODE_VA, 2 * 4096).is_err()
        || vm_c.reserve_existing(CODE_VA, 2 * 4096).is_err()
    {
        let _ = vm_a.try_destroy();
        let _ = vm_b.try_destroy();
        let _ = vm_c.try_destroy();
        PROCESS_PROBE_STARTED.store(false, Ordering::Release);
        return None;
    }
    *PROCESS_PROBE_SPACE_A.lock() = Some(vm_a);
    *PROCESS_PROBE_SPACE_B.lock() = Some(vm_b);
    *PROCESS_PROBE_SPACE_C.lock() = Some(vm_c);

    let context_a = CpuContext {
        rip: entry_a,
        rsp: user_stack_a,
        rflags: 0x202,
        ..CpuContext::default()
    };
    let user_probe_security = carlio_os::process::ProcessSecurityContext::new(
        Credentials {
            uid: 1000,
            gid: 1000,
        },
        &[],
        0o022,
    )?;
    let first = PROCESS_SCHEDULER
        .create_with_stack_pool_and_security(
            &PROCESS_KERNEL_STACKS,
            root_a,
            context_a,
            user_probe_security,
        )
        .ok()?;
    let context_b = CpuContext {
        rip: entry_b,
        rsp: user_stack_b,
        rflags: 0x202,
        ..CpuContext::default()
    };
    let second =
        match PROCESS_SCHEDULER.create_with_stack_pool(&PROCESS_KERNEL_STACKS, root_b, context_b) {
            Ok(id) => id,
            Err(_) => {
                // No process has run yet, so its stack is safe to reclaim.
                let _ = PROCESS_SCHEDULER.reap(first, &PROCESS_KERNEL_STACKS);
                PROCESS_PROBE_STARTED.store(false, Ordering::Release);
                return None;
            }
        };
    let context_c = CpuContext {
        rip: entry_c,
        rsp: user_stack_c,
        rflags: 0x202,
        ..CpuContext::default()
    };
    let third =
        match PROCESS_SCHEDULER.create_with_stack_pool(&PROCESS_KERNEL_STACKS, root_c, context_c) {
            Ok(id) => id,
            Err(_) => {
                let _ = PROCESS_SCHEDULER.reap(second, &PROCESS_KERNEL_STACKS);
                let _ = PROCESS_SCHEDULER.reap(first, &PROCESS_KERNEL_STACKS);
                PROCESS_PROBE_STARTED.store(false, Ordering::Release);
                return None;
            }
        };
    PROCESS_PROBE_IDS[0].store(first, Ordering::Release);
    PROCESS_PROBE_IDS[1].store(second, Ordering::Release);
    PROCESS_PROBE_IDS[2].store(third, Ordering::Release);
    let bsp_cpu = current_scheduler_cpu();
    let mut secondary_cpus = [usize::MAX; 2];
    let mut secondary_count = 0usize;
    for cpu in 0..SMP_DETECTED_COUNT.load(Ordering::Acquire) {
        if BOOT_CPU_LOCALS[cpu].online.load(Ordering::Acquire) != 0
            && is_secondary_cpu(cpu)
            && secondary_count < secondary_cpus.len()
        {
            secondary_cpus[secondary_count] = cpu;
            secondary_count += 1;
        }
    }
    let pinned = PROCESS_SCHEDULER.pin_ready_to_cpu(first, bsp_cpu).is_ok()
        && secondary_count == secondary_cpus.len()
        && PROCESS_SCHEDULER
            .pin_ready_to_cpu(second, secondary_cpus[0])
            .is_ok()
        && PROCESS_SCHEDULER
            .pin_ready_to_cpu(third, secondary_cpus[1])
            .is_ok();
    if !pinned {
        write("Runtime: afinidade inicial das tarefas CPL3 não pôde ser aplicada.\r\n");
        let _ = reap_process_probe();
        PROCESS_PROBE_STARTED.store(false, Ordering::Release);
        return None;
    }
    for id in [first, second, third] {
        let process = PROCESS_SCHEDULER.process(id)?;
        let frame =
            unsafe { idt::initialize_user_frame(process.kernel_stack_top, process.context) }?;
        if frame as u64 != process.interrupt_frame {
            PROCESS_PROBE_STARTED.store(false, Ordering::Release);
            return None;
        }
    }
    let cpu = current_scheduler_cpu();
    let process = schedule_process_on_cpu(cpu).ok()??;
    if process.page_table != root {
        PROCESS_PROBE_CR3_SWITCHES.fetch_add(1, Ordering::AcqRel);
    }
    if !unsafe {
        idt::activate_cpu_process_address_space(
            current_scheduler_cpu(),
            process.page_table,
            process.kernel_stack_top,
        )
    } {
        PROCESS_PROBE_STARTED.store(false, Ordering::Release);
        return None;
    }
    AP_PROCESS_SCHEDULING.store(true, Ordering::Release);
    write("Runtime: tarefas com CR3 independentes despachadas.\r\n");
    Some(process.interrupt_frame as *mut idt::UserInterruptFrame)
}

#[cfg(target_os = "none")]
fn make_process_probe_space(
    kernel_root: u64,
    stack_va: u64,
    path: &str,
    argv: &[&str],
    env: &[&str],
) -> Option<(
    OwnedAddressSpace<BootPageTableAccess, PROCESS_VM_PAGE_CAPACITY>,
    u64,
    u64,
)> {
    let mut target = match ElfAddressSpace::<PROCESS_VM_PAGE_CAPACITY>::new(kernel_root) {
        Ok(space) => space,
        Err(_) => {
            write("Runtime: nova raiz de paginação recusada.\r\n");
            return None;
        }
    };
    let context = AccessContext::new(Credentials { uid: 0, gid: 0 }, &[], 0);
    let mut scratch = ELF_LOAD_SCRATCH.lock();
    let mut interpreter_scratch = ELF_INTERPRETER_SCRATCH.lock();
    let image = {
        let vfs = PROCESS_VFS.lock();
        match carlio_os::elf::load_interpreted_executable_from_vfs::<64, PROCESS_VFS_CAPACITY, _>(
            &vfs,
            path,
            context,
            &mut scratch[..],
            &mut interpreter_scratch[..],
            &mut target,
        ) {
            Ok(image) => image,
            Err(_) => {
                write("Runtime: ELF do VFS recusado por acesso, formato ou mapeamento.\r\n");
                let _ = target.0.try_destroy();
                return None;
            }
        }
    };
    if target
        .0
        .map_zeroed(
            stack_va,
            1 | PAGE_FLAG_USER | PAGE_FLAG_WRITABLE | PAGE_FLAG_NO_EXECUTE,
        )
        .is_err()
    {
        write("Runtime: falha ao mapear stack CPL3.\r\n");
        let _ = target.0.try_destroy();
        return None;
    }
    let stack_top = stack_va + 4096;
    let auxv = image.auxv();
    let stack = match carlio_os::elf::build_initial_user_stack_with_auxv(
        &mut target,
        stack_top,
        argv,
        env,
        &auxv,
    ) {
        Ok(stack) => stack,
        Err(carlio_os::elf::ElfError::InvalidArguments) => {
            write("Runtime: argumentos excedem a stack inicial CPL3.\r\n");
            let _ = target.0.try_destroy();
            return None;
        }
        Err(_) => {
            write("Runtime: escrita da stack inicial CPL3 falhou.\r\n");
            let _ = target.0.try_destroy();
            return None;
        }
    };
    Some((target.0, image.entry, stack.stack_pointer))
}

#[cfg(target_os = "none")]
fn install_process_probe_images(first: &[u8], second: &[u8]) -> bool {
    let root = Credentials { uid: 0, gid: 0 };
    let access = AccessContext::new(root, &[], 0);
    let mut vfs = PROCESS_VFS.lock();
    if vfs
        .check_access_as("/bin", access, carlio_os::vfs::Access::READ)
        .is_err()
        && vfs.create_fhs_layout().is_err()
    {
        return false;
    }
    if vfs
        .check_access_as("/programas", access, carlio_os::vfs::Access::READ)
        .is_err()
        && vfs.create_dir("/", "programas", root, 0o755).is_err()
    {
        return false;
    }
    let _ = vfs.remove_file_as("/programas/probe", access);
    let _ = vfs.remove_file_as("/programas/fault", access);
    let _ = vfs.remove_file_as("/programas/restrito", access);
    if vfs.create_file("/programas", "probe", root, 0o755).is_err()
        || vfs.create_file("/programas", "fault", root, 0o755).is_err()
        || vfs
            .create_file("/programas", "restrito", root, 0o600)
            .is_err()
    {
        return false;
    }
    vfs.write_file_as("/programas/probe", access, first).is_ok()
        && vfs
            .write_file_as("/programas/fault", access, second)
            .is_ok()
}

#[cfg(target_os = "none")]
fn probe_elf_image(task: &[u8]) -> [u8; 512] {
    use carlio_os::elf::{ELF_HEADER_SIZE, PF_R, PF_X, PROGRAM_HEADER_SIZE, PT_LOAD};
    let mut elf = [0u8; 512];
    elf[..4].copy_from_slice(b"\x7fELF");
    elf[4] = 2;
    elf[5] = 1;
    elf[6] = 1;
    elf[16..18].copy_from_slice(&2u16.to_le_bytes());
    elf[18..20].copy_from_slice(&62u16.to_le_bytes());
    elf[20..24].copy_from_slice(&1u32.to_le_bytes());
    elf[24..32].copy_from_slice(&0x0000_4000_0000_3000u64.to_le_bytes());
    elf[32..40].copy_from_slice(&(ELF_HEADER_SIZE as u64).to_le_bytes());
    elf[52..54].copy_from_slice(&(ELF_HEADER_SIZE as u16).to_le_bytes());
    elf[54..56].copy_from_slice(&(PROGRAM_HEADER_SIZE as u16).to_le_bytes());
    elf[56..58].copy_from_slice(&1u16.to_le_bytes());
    let ph = ELF_HEADER_SIZE;
    elf[ph..ph + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
    elf[ph + 4..ph + 8].copy_from_slice(&(PF_R | PF_X).to_le_bytes());
    elf[ph + 8..ph + 16].copy_from_slice(&120u64.to_le_bytes());
    elf[ph + 16..ph + 24].copy_from_slice(&0x0000_4000_0000_3000u64.to_le_bytes());
    elf[ph + 32..ph + 40].copy_from_slice(&(task.len() as u64 + 2).to_le_bytes());
    elf[ph + 40..ph + 48].copy_from_slice(&(task.len() as u64 + 2).to_le_bytes());
    elf[ph + 48..ph + 56].copy_from_slice(&1u64.to_le_bytes());
    elf[120..120 + task.len()].copy_from_slice(task);
    elf[120 + task.len()] = 0xcd;
    elf[121 + task.len()] = 0x80;
    elf
}

#[cfg(target_os = "none")]
pub(crate) fn process_probe_exit(code: u64) -> Option<*mut idt::UserInterruptFrame> {
    let cpu = current_scheduler_cpu();
    if PROCESS_SCHEDULER.exit(cpu, code as i64).is_err() {
        return None;
    }
    let process = schedule_process_on_cpu(cpu).ok()??;
    if !unsafe {
        idt::activate_cpu_process_address_space(cpu, process.page_table, process.kernel_stack_top)
    } {
        return None;
    }
    Some(process.interrupt_frame as *mut idt::UserInterruptFrame)
}

#[cfg(target_os = "none")]
pub(crate) fn process_runtime_fault(
    vector: u64,
    error_code: u64,
    cr2: u64,
) -> Option<*mut idt::UserInterruptFrame> {
    write("Runtime: fault CPL3 vetorizado=");
    write_u64(vector);
    if vector == 14 {
        write(" CR2=0x");
        write_hex_u64(cr2);
    }
    write("; encerrando somente a tarefa.\r\n");
    PROCESS_PROBE_FAULTS.fetch_add(1, Ordering::AcqRel);
    let cpu = current_scheduler_cpu();
    PROCESS_SCHEDULER
        .fault(cpu, vector as u8, error_code, cr2)
        .ok()?;
    let process = schedule_process_on_cpu(cpu).ok()??;
    if !unsafe {
        idt::activate_cpu_process_address_space(cpu, process.page_table, process.kernel_stack_top)
    } {
        return None;
    }
    Some(process.interrupt_frame as *mut idt::UserInterruptFrame)
}

#[cfg(target_os = "none")]
fn linux_syscall_probe_task() -> ([u8; 512], usize) {
    const CODE_VA: u64 = 0x0000_4000_0000_3000;
    const STACK_VA: u64 = CODE_VA + 4096;
    const MESSAGE: &[u8] = b"write ok\n";
    const PATH: &[u8] = b"/programas/probe\0";
    let mut code = [0u8; 512];
    let mut length = 0usize;
    // Pequena janela para os APs entrarem simultaneamente em CPL3. O contador
    // anterior (67 milhões) podia exceder o watchdog de 5 s sob QEMU TCG antes
    // de qualquer syscall e transformar lentidão previsível da fixture em falso
    // travamento do kernel.
    append_probe_bytes(&mut code, &mut length, &[0xb9, 0x00, 0x00, 0x10, 0x00]);
    append_probe_bytes(&mut code, &mut length, &[0xe2, 0xfe]); // loop
    append_probe_bytes(&mut code, &mut length, &[0xbf, 1, 0, 0, 0]); // write(1, ...)
    append_probe_bytes(&mut code, &mut length, &[0x48, 0xbe]);
    let message_pointer_offset = length;
    append_probe_bytes(&mut code, &mut length, &[0; 8]);
    append_probe_bytes(
        &mut code,
        &mut length,
        &[
            0xba,
            MESSAGE.len() as u8,
            0,
            0,
            0,
            0xb8,
            1,
            0,
            0,
            0,
            0x0f,
            0x05,
        ],
    );
    for number in [
        carlio_os::linux_abi::SYS_GETUID,
        carlio_os::linux_abi::SYS_GETEUID,
        carlio_os::linux_abi::SYS_GETGID,
        carlio_os::linux_abi::SYS_GETEGID,
    ] {
        append_probe_syscall(&mut code, &mut length, number);
    }
    append_probe_bytes(&mut code, &mut length, &[0x31, 0xff, 0x31, 0xf6]);
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_GETGROUPS);
    // openat(AT_FDCWD, "/programas/probe", O_RDONLY), then read one byte
    // into writable user stack memory and close the descriptor.
    append_probe_bytes(&mut code, &mut length, &[0x48, 0xbf]);
    append_probe_bytes(&mut code, &mut length, &((-100i64) as u64).to_le_bytes());
    append_probe_bytes(&mut code, &mut length, &[0x48, 0xbe]);
    let path_pointer_offset = length;
    append_probe_bytes(&mut code, &mut length, &[0; 8]);
    append_probe_bytes(&mut code, &mut length, &[0x31, 0xd2]);
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_OPENAT);
    append_probe_bytes(&mut code, &mut length, &[0x48, 0x89, 0xc7, 0x48, 0xbe]);
    append_probe_bytes(&mut code, &mut length, &(STACK_VA + 2048).to_le_bytes());
    append_probe_bytes(&mut code, &mut length, &[0xba, 1, 0, 0, 0]);
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_READ);
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_CLOSE);
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_GETPID);
    append_probe_bytes(&mut code, &mut length, &[0x31, 0xff, 0x48, 0xbe]);
    append_probe_bytes(
        &mut code,
        &mut length,
        &0x0000_7000_0000_0000u64.to_le_bytes(),
    );
    append_probe_bytes(&mut code, &mut length, &[0xba, 1, 0, 0, 0]);
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_READ);
    // read(0) positivo: o harness QEMU envia um byte após o aviso do kernel.
    append_probe_bytes(&mut code, &mut length, &[0xbf, 0, 0, 0, 0, 0x48, 0xbe]);
    append_probe_bytes(&mut code, &mut length, &(STACK_VA + 2050).to_le_bytes());
    append_probe_bytes(&mut code, &mut length, &[0xba, 1, 0, 0, 0]);
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_READ);
    // mmap an anonymous private page, write through the returned mapping, and
    // release it again through munmap.
    append_probe_bytes(
        &mut code,
        &mut length,
        &[
            0x31, 0xff, // rdi = hint NULL
            0xbe, 0x00, 0x10, 0, 0, // rsi = 4096
            0xba, 3, 0, 0, 0, // rdx = PROT_READ | PROT_WRITE
            0x41, 0xba, 0x22, 0, 0, 0, // r10 = MAP_PRIVATE | MAP_ANONYMOUS
            0x49, 0xc7, 0xc0, 0xff, 0xff, 0xff, 0xff, // r8 = -1
            0x45, 0x31, 0xc9, // r9 = 0
        ],
    );
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_MMAP);
    append_probe_bytes(&mut code, &mut length, &[0xc6, 0x00, 0x51]); // *rax = 'Q'
    append_probe_bytes(&mut code, &mut length, &[0x48, 0x89, 0xc7]); // rdi = mapping
    append_probe_bytes(&mut code, &mut length, &[0xbe, 0x00, 0x10, 0, 0]);
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_MUNMAP);
    // An unprivileged process must receive EACCES for a root-owned 0600 file.
    append_probe_bytes(&mut code, &mut length, &[0x48, 0xbf]);
    append_probe_bytes(&mut code, &mut length, &((-100i64) as u64).to_le_bytes());
    append_probe_bytes(&mut code, &mut length, &[0x48, 0xbe]);
    let denied_path_pointer_offset = length;
    append_probe_bytes(&mut code, &mut length, &[0; 8]);
    append_probe_bytes(&mut code, &mut length, &[0x31, 0xd2]);
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_OPENAT);
    append_probe_bytes(&mut code, &mut length, &[0x31, 0xff]);
    append_probe_syscall(&mut code, &mut length, carlio_os::linux_abi::SYS_EXIT);
    let message_offset = length;
    append_probe_bytes(&mut code, &mut length, MESSAGE);
    let path_offset = length;
    append_probe_bytes(&mut code, &mut length, PATH);
    let denied_path_offset = length;
    append_probe_bytes(&mut code, &mut length, b"/programas/restrito\0");
    code[message_pointer_offset..message_pointer_offset + 8]
        .copy_from_slice(&(CODE_VA + message_offset as u64).to_le_bytes());
    code[path_pointer_offset..path_pointer_offset + 8]
        .copy_from_slice(&(CODE_VA + path_offset as u64).to_le_bytes());
    code[denied_path_pointer_offset..denied_path_pointer_offset + 8]
        .copy_from_slice(&(CODE_VA + denied_path_offset as u64).to_le_bytes());
    (code, length)
}

#[cfg(target_os = "none")]
fn append_probe_syscall(code: &mut [u8; 512], length: &mut usize, number: u64) {
    append_probe_bytes(code, length, &[0xb8]); // mov eax, imm32
    append_probe_bytes(code, length, &(number as u32).to_le_bytes());
    append_probe_bytes(code, length, &[0x0f, 0x05]);
}

#[cfg(target_os = "none")]
fn append_probe_bytes(code: &mut [u8; 512], length: &mut usize, bytes: &[u8]) {
    let end = *length + bytes.len();
    code[*length..end].copy_from_slice(bytes);
    *length = end;
}

#[cfg(target_os = "none")]
fn cpu_index_for_lapic(lapic_id: u32) -> Option<usize> {
    BOOT_CPU_LOCALS.iter().position(|cpu| {
        cpu.online.load(Ordering::Acquire) != 0 && cpu.lapic_id.load(Ordering::Relaxed) == lapic_id
    })
}

#[cfg(target_os = "none")]
fn current_scheduler_cpu() -> usize {
    lapic::local_id().and_then(cpu_index_for_lapic).unwrap_or(0)
}

#[cfg(target_os = "none")]
fn schedule_process_on_cpu(
    cpu: usize,
) -> Result<Option<carlio_os::process::Process>, carlio_os::process::ProcessError> {
    if is_secondary_cpu(cpu) {
        PROCESS_SCHEDULER.schedule_local(cpu)
    } else {
        PROCESS_SCHEDULER.schedule(cpu)
    }
}

#[cfg(target_os = "none")]
pub(crate) fn scheduler_has_current_process(cpu: usize) -> bool {
    PROCESS_SCHEDULER.current(cpu).is_some()
}

#[cfg(target_os = "none")]
pub(crate) fn dispatch_linux_syscall(
    number: u64,
    arguments: [u64; 6],
    cpu: usize,
) -> Option<carlio_os::linux_abi::SyscallOutcome> {
    let process = PROCESS_SCHEDULER.current(cpu)?;
    let mut io = KernelLinuxSyscallIo { process };
    let program_break = io.with_process_vm(|vm| Ok(vm.program_break())).unwrap_or(0);
    let outcome = carlio_os::linux_abi::dispatch(
        number,
        arguments,
        carlio_os::linux_abi::ProcessContext {
            process_id: process.id,
            uid: process.security.credentials.uid,
            gid: process.security.credentials.gid,
            supplementary_groups: process.security.supplementary_groups,
            group_count: process.security.group_count,
            program_break,
        },
        &mut io,
    );
    if number == carlio_os::linux_abi::SYS_OPENAT && process.security.credentials.uid != 0 {
        if let carlio_os::linux_abi::SyscallOutcome::Return(value) = outcome {
            LINUX_VFS_DENIAL_RESULT.store(value, Ordering::Release);
            if value == (-(carlio_os::linux_abi::EACCES as i64)) as u64 {
                LINUX_VFS_DENIAL_PROBE_PASSED.store(true, Ordering::Release);
            }
        }
    }
    if number == carlio_os::linux_abi::SYS_MUNMAP {
        if let carlio_os::linux_abi::SyscallOutcome::Return(value) = outcome {
            LINUX_MUNMAP_RESULT.store(value, Ordering::Release);
        }
    }
    let identity_bit = match number {
        carlio_os::linux_abi::SYS_GETUID
            if outcome
                == carlio_os::linux_abi::SyscallOutcome::Return(
                    process.security.credentials.uid as u64,
                ) =>
        {
            1
        }
        carlio_os::linux_abi::SYS_GETEUID
            if outcome
                == carlio_os::linux_abi::SyscallOutcome::Return(
                    process.security.credentials.uid as u64,
                ) =>
        {
            2
        }
        carlio_os::linux_abi::SYS_GETGID
            if outcome
                == carlio_os::linux_abi::SyscallOutcome::Return(
                    process.security.credentials.gid as u64,
                ) =>
        {
            4
        }
        carlio_os::linux_abi::SYS_GETEGID
            if outcome
                == carlio_os::linux_abi::SyscallOutcome::Return(
                    process.security.credentials.gid as u64,
                ) =>
        {
            8
        }
        carlio_os::linux_abi::SYS_GETGROUPS
            if arguments[0] == 0
                && outcome
                    == carlio_os::linux_abi::SyscallOutcome::Return(
                        process.security.group_count as u64,
                    ) =>
        {
            16
        }
        carlio_os::linux_abi::SYS_WRITE
            if arguments[0] == 1
                && outcome == carlio_os::linux_abi::SyscallOutcome::Return(arguments[2]) =>
        {
            32
        }
        carlio_os::linux_abi::SYS_READ
            if arguments[0] == 0
                && arguments[2] == 1
                && outcome
                    == carlio_os::linux_abi::SyscallOutcome::Return(
                        (-(carlio_os::linux_abi::EFAULT as i64)) as u64,
                    ) =>
        {
            64
        }
        carlio_os::linux_abi::SYS_READ
            if arguments[0] >= carlio_os::open_files::FILE_FD_BASE as u64
                && matches!(outcome, carlio_os::linux_abi::SyscallOutcome::Return(amount) if amount > 0) =>
        {
            128
        }
        carlio_os::linux_abi::SYS_READ
            if arguments[0] == 0
                && arguments[1] == 0x0000_4000_0000_4802
                && arguments[2] == 1
                && matches!(outcome, carlio_os::linux_abi::SyscallOutcome::Return(1)) =>
        {
            256
        }
        _ => 0,
    };
    if identity_bit != 0 {
        LINUX_CREDENTIAL_PROBE_MASK.fetch_or(identity_bit, Ordering::AcqRel);
    }
    if number == carlio_os::linux_abi::SYS_GETPID
        && outcome == carlio_os::linux_abi::SyscallOutcome::Return(process.id)
    {
        LINUX_GETPID_PROBE_PASSED.store(true, Ordering::Release);
    }
    match (number, outcome) {
        (carlio_os::linux_abi::SYS_MMAP, carlio_os::linux_abi::SyscallOutcome::Return(address))
            if io
                .with_process_vm(|vm| {
                    vm.mapping(address)
                        .map(|mapping| {
                            mapping.flags & PAGE_FLAG_USER != 0
                                && mapping.flags & PAGE_FLAG_WRITABLE != 0
                        })
                        .map_err(|_| carlio_os::linux_abi::EFAULT)
                })
                .unwrap_or(false) =>
        {
            LINUX_MEMORY_PROBE_MASK.fetch_or(0x1, Ordering::AcqRel);
        }
        (carlio_os::linux_abi::SYS_MUNMAP, carlio_os::linux_abi::SyscallOutcome::Return(0))
            if io
                .with_process_vm(|vm| Ok(vm.mapping(arguments[0]) == Err(FrameError::NotMapped)))
                .unwrap_or(false) =>
        {
            LINUX_MEMORY_PROBE_MASK.fetch_or(0x2, Ordering::AcqRel);
        }
        _ => {}
    }
    Some(outcome)
}

#[cfg(target_os = "none")]
struct KernelLinuxSyscallIo {
    process: carlio_os::process::Process,
}

#[cfg(target_os = "none")]
impl KernelLinuxSyscallIo {
    fn with_process_vm<R>(
        &mut self,
        operation: impl FnOnce(&mut RuntimeProcessVm) -> Result<R, u16>,
    ) -> Result<R, u16> {
        let slot = if self.process.id == PROCESS_PROBE_IDS[0].load(Ordering::Acquire) {
            &PROCESS_PROBE_SPACE_A
        } else if self.process.id == PROCESS_PROBE_IDS[1].load(Ordering::Acquire) {
            &PROCESS_PROBE_SPACE_B
        } else if self.process.id == PROCESS_PROBE_IDS[2].load(Ordering::Acquire) {
            &PROCESS_PROBE_SPACE_C
        } else {
            return Err(carlio_os::linux_abi::ENOMEM);
        };
        let mut guard = slot.lock();
        let vm = guard.as_mut().ok_or(carlio_os::linux_abi::ENOMEM)?;
        operation(vm)
    }
}

#[cfg(target_os = "none")]
impl carlio_os::linux_abi::LinuxSyscallIo for KernelLinuxSyscallIo {
    fn clock_gettime(
        &mut self,
        clock_id: u64,
    ) -> Result<carlio_os::linux_abi::Timespec, u16> {
        if clock_id != carlio_os::linux_abi::CLOCK_MONOTONIC {
            return Err(carlio_os::linux_abi::EINVAL);
        }
        let ticks = idt::uptime_ticks();
        let seconds = i64::try_from(ticks / idt::TICK_RATE_HZ)
            .map_err(|_| carlio_os::linux_abi::EOVERFLOW)?;
        let subsecond_ticks = ticks % idt::TICK_RATE_HZ;
        let nanoseconds = i64::try_from(subsecond_ticks)
            .ok()
            .and_then(|value| value.checked_mul(1_000_000_000))
            .and_then(|value| value.checked_div(i64::try_from(idt::TICK_RATE_HZ).ok()?))
            .ok_or(carlio_os::linux_abi::EOVERFLOW)?;
        Ok(carlio_os::linux_abi::Timespec {
            seconds,
            nanoseconds,
        })
    }

    fn clock_getres(
        &mut self,
        clock_id: u64,
    ) -> Result<carlio_os::linux_abi::Timespec, u16> {
        if clock_id != carlio_os::linux_abi::CLOCK_MONOTONIC {
            return Err(carlio_os::linux_abi::EINVAL);
        }
        Ok(carlio_os::linux_abi::Timespec {
            seconds: 0,
            nanoseconds: i64::try_from(1_000_000_000 / idt::TICK_RATE_HZ)
                .map_err(|_| carlio_os::linux_abi::EOVERFLOW)?,
        })
    }

    fn mmap(
        &mut self,
        address: u64,
        length: u64,
        protection: u64,
        flags: u64,
        descriptor: u64,
        offset: u64,
    ) -> Result<u64, u16> {
        self.with_process_vm(|vm| {
            vm.mmap_linux(address, length, protection, flags, descriptor, offset)
                .map_err(process_vm_errno)
        })
    }

    fn munmap(&mut self, address: u64, length: u64) -> Result<(), u16> {
        self.with_process_vm(|vm| vm.munmap(address, length).map_err(process_vm_errno))
    }

    fn brk(&mut self, current: u64, requested: u64) -> u64 {
        self.with_process_vm(|vm| vm.brk(requested).map_err(process_vm_errno))
            .unwrap_or(current)
    }

    fn copy_from_user(&mut self, address: u64, destination: &mut [u8]) -> Result<(), u16> {
        let hhdm = unsafe { core::ptr::addr_of!(HHDM_OFFSET).read_volatile() };
        let lookup = PageTableManager::new(BootPageTableAccess { hhdm }, self.process.page_table)
            .map_err(|_| carlio_os::linux_abi::EFAULT)?;
        unsafe { carlio_os::user_memory::copy_from_user(&lookup, address, destination) }
            .map_err(|_| carlio_os::linux_abi::EFAULT)
    }

    fn copy_to_user(&mut self, address: u64, source: &[u8]) -> Result<(), u16> {
        let hhdm = unsafe { core::ptr::addr_of!(HHDM_OFFSET).read_volatile() };
        let lookup = PageTableManager::new(BootPageTableAccess { hhdm }, self.process.page_table)
            .map_err(|_| carlio_os::linux_abi::EFAULT)?;
        unsafe { carlio_os::user_memory::copy_to_user(&lookup, address, source) }
            .map_err(|_| carlio_os::linux_abi::EFAULT)
    }

    fn validate_user_write(&mut self, address: u64, length: usize) -> Result<(), u16> {
        let hhdm = unsafe { core::ptr::addr_of!(HHDM_OFFSET).read_volatile() };
        let lookup = PageTableManager::new(BootPageTableAccess { hhdm }, self.process.page_table)
            .map_err(|_| carlio_os::linux_abi::EFAULT)?;
        carlio_os::user_memory::validate(&lookup, address, length, true)
            .map_err(|_| carlio_os::linux_abi::EFAULT)
    }

    fn read(&mut self, descriptor: u64, destination: &mut [u8]) -> Result<usize, u16> {
        if descriptor == 0 {
            write("ABI Linux: probe CPL3 aguardando byte de entrada serial.\r\n");
            for (index, output) in destination.iter_mut().enumerate() {
                let byte = match serial_read() {
                    Ok(byte) => byte,
                    Err(_) if index > 0 => return Ok(index),
                    Err(_) => return Err(carlio_os::linux_abi::EIO),
                };
                *output = if byte == b'\r' { b'\n' } else { byte };
                if *output == b'\n' {
                    return Ok(index + 1);
                }
            }
            return Ok(destination.len());
        }
        let descriptor = u32::try_from(descriptor).map_err(|_| carlio_os::linux_abi::EBADF)?;
        let file = OPEN_FILES
            .lock()
            .get(self.process.id, descriptor)
            .map_err(|_| carlio_os::linux_abi::EBADF)?;
        if !file.readable {
            return Err(carlio_os::linux_abi::EBADF);
        }
        let path = file.path_str().ok_or(carlio_os::linux_abi::EIO)?;
        let mut scratch = [0u8; 256];
        let chunk = destination.len().min(scratch.len());
        let amount = PROCESS_VFS
            .lock()
            .read_file_range_as(
                path,
                self.process.security.access_context(),
                usize::try_from(file.offset).map_err(|_| carlio_os::linux_abi::EINVAL)?,
                &mut scratch[..chunk],
            )
            .map_err(vfs_errno)?;
        destination[..amount].copy_from_slice(&scratch[..amount]);
        OPEN_FILES
            .lock()
            .set_offset(self.process.id, descriptor, file.offset + amount as u64)
            .map_err(|_| carlio_os::linux_abi::EBADF)?;
        Ok(amount)
    }

    fn open(&mut self, path: &str, flags: u64, _mode: u64) -> Result<u64, u16> {
        let _transaction = ROOTFS_TRANSACTION_LOCK.lock();
        let access = match flags & 3 {
            0 => carlio_os::vfs::Access::READ,
            1 => carlio_os::vfs::Access::WRITE,
            2 => carlio_os::vfs::Access::READ_WRITE,
            _ => return Err(carlio_os::linux_abi::EINVAL),
        };
        let readable = flags & 3 != 1;
        let writable = flags & 3 != 0;
        let context = self.process.security.access_context();
        let mut vfs = PROCESS_VFS.lock();
        let mut changed = false;
        match vfs.check_access_as(path, context, access) {
            Ok(carlio_os::vfs::NodeKind::File) => {
                if flags & 0x80 != 0 && flags & 0x40 != 0 {
                    return Err(carlio_os::linux_abi::EEXIST);
                }
            }
            Ok(carlio_os::vfs::NodeKind::Directory) => return Err(carlio_os::linux_abi::EISDIR),
            Err(carlio_os::vfs::VfsError::NotFound) if flags & 0x40 != 0 => {
                let (parent, name) = path.rsplit_once('/').ok_or(carlio_os::linux_abi::EINVAL)?;
                if name.is_empty() {
                    return Err(carlio_os::linux_abi::EINVAL);
                }
                vfs.create_file_as(
                    if parent.is_empty() { "/" } else { parent },
                    name,
                    context,
                    _mode as u16,
                )
                .map_err(vfs_errno)?;
                changed = true;
            }
            Err(error) => return Err(vfs_errno(error)),
        }
        if flags & 0x200 != 0 {
            vfs.truncate_file_as(path, context).map_err(vfs_errno)?;
            changed = true;
        }
        let append = flags & 0x400 != 0;
        let initial_offset = if append {
            vfs.file_len_as(path, context, carlio_os::vfs::Access::WRITE)
                .map_err(vfs_errno)? as u64
        } else {
            0
        };
        drop(vfs);
        if changed {
            persist_linux_rootfs()?;
        }
        let mut files = OPEN_FILES.lock();
        let descriptor = files
            .open(self.process.id, path, readable, writable, append)
            .map_err(|_| carlio_os::linux_abi::ENFILE)?;
        if initial_offset != 0 {
            files
                .set_offset(self.process.id, descriptor, initial_offset)
                .map_err(|_| carlio_os::linux_abi::EBADF)?;
        }
        Ok(descriptor as u64)
    }

    fn write(&mut self, descriptor: u64, bytes: &[u8]) -> Result<usize, u16> {
        if descriptor == 1 || descriptor == 2 {
            for byte in bytes {
                if !serial_write(*byte) {
                    return Err(carlio_os::linux_abi::EIO);
                }
            }
            return Ok(bytes.len());
        }
        let descriptor = u32::try_from(descriptor).map_err(|_| carlio_os::linux_abi::EBADF)?;
        let _transaction = ROOTFS_TRANSACTION_LOCK.lock();
        let file = OPEN_FILES
            .lock()
            .get(self.process.id, descriptor)
            .map_err(|_| carlio_os::linux_abi::EBADF)?;
        if !file.writable {
            return Err(carlio_os::linux_abi::EBADF);
        }
        let path = file.path_str().ok_or(carlio_os::linux_abi::EIO)?;
        let context = self.process.security.access_context();
        let mut vfs = PROCESS_VFS.lock();
        let offset = if file.append {
            vfs.file_len_as(path, context, carlio_os::vfs::Access::WRITE)
                .map_err(vfs_errno)? as u64
        } else {
            file.offset
        };
        let amount = vfs
            .write_file_range_as(
                path,
                context,
                usize::try_from(offset).map_err(|_| carlio_os::linux_abi::EFBIG)?,
                bytes,
            )
            .map_err(vfs_errno)?;
        drop(vfs);
        persist_linux_rootfs()?;
        OPEN_FILES
            .lock()
            .set_offset(self.process.id, descriptor, offset + amount as u64)
            .map_err(|_| carlio_os::linux_abi::EBADF)?;
        Ok(amount)
    }

    fn close(&mut self, descriptor: u64) -> Result<(), u16> {
        let descriptor = u32::try_from(descriptor).map_err(|_| carlio_os::linux_abi::EBADF)?;
        if descriptor >= carlio_os::open_files::FILE_FD_BASE {
            return OPEN_FILES
                .lock()
                .close(self.process.id, descriptor)
                .map_err(|_| carlio_os::linux_abi::EBADF);
        }
        NETWORK_SOCKETS
            .lock()
            .close(self.process.id, descriptor)
            .map_err(network_errno)
    }

    fn seek(&mut self, descriptor: u64, offset: i64, whence: u64) -> Result<u64, u16> {
        let descriptor = u32::try_from(descriptor).map_err(|_| carlio_os::linux_abi::EBADF)?;
        let file = OPEN_FILES
            .lock()
            .get(self.process.id, descriptor)
            .map_err(|_| carlio_os::linux_abi::EBADF)?;
        let base = match whence {
            0 => 0i64,
            1 => i64::try_from(file.offset).map_err(|_| carlio_os::linux_abi::EINVAL)?,
            2 => {
                let access = if file.readable {
                    carlio_os::vfs::Access::READ
                } else {
                    carlio_os::vfs::Access::WRITE
                };
                PROCESS_VFS
                    .lock()
                    .file_len_as(
                        file.path_str().ok_or(carlio_os::linux_abi::EIO)?,
                        self.process.security.access_context(),
                        access,
                    )
                    .map_err(vfs_errno)? as i64
            }
            _ => return Err(carlio_os::linux_abi::EINVAL),
        };
        let next = base
            .checked_add(offset)
            .filter(|value| *value >= 0)
            .ok_or(carlio_os::linux_abi::EINVAL)? as u64;
        OPEN_FILES
            .lock()
            .set_offset(self.process.id, descriptor, next)
            .map_err(|_| carlio_os::linux_abi::EBADF)?;
        Ok(next)
    }

    fn socket(&mut self, domain: u64, kind: u64, protocol: u64) -> Result<u64, u16> {
        NETWORK_SOCKETS
            .lock()
            .create(
                self.process.id,
                self.process.security.credentials.uid,
                domain,
                kind,
                protocol,
            )
            .map(|fd| fd as u64)
            .map_err(network_errno)
    }

    fn bind(&mut self, descriptor: u64, address: [u8; 4], port: u16) -> Result<(), u16> {
        let descriptor = u32::try_from(descriptor).map_err(|_| carlio_os::linux_abi::EBADF)?;
        NETWORK_SOCKETS
            .lock()
            .bind(
                self.process.id,
                self.process.security.credentials.uid,
                descriptor,
                address,
                port,
            )
            .map_err(network_errno)
    }

    fn listen(&mut self, descriptor: u64, backlog: u16) -> Result<(), u16> {
        let descriptor = u32::try_from(descriptor).map_err(|_| carlio_os::linux_abi::EBADF)?;
        NETWORK_SOCKETS
            .lock()
            .listen(self.process.id, descriptor, backlog)
            .map_err(network_errno)
    }
}

#[cfg(target_os = "none")]
fn process_vm_errno(error: carlio_os::vma::ProcessVmError) -> u16 {
    use carlio_os::vma::{ProcessVmError as E, VmaError as V};
    match error {
        E::Vma(V::InvalidRange | V::Overlap) => carlio_os::linux_abi::EINVAL,
        E::Vma(V::Capacity | V::HeapCollision) | E::Memory(FrameError::CapacityExceeded) => {
            carlio_os::linux_abi::ENOMEM
        }
        E::Vma(V::NotMapped) => carlio_os::linux_abi::EINVAL,
        E::UnsupportedMapping => carlio_os::linux_abi::EOPNOTSUPP,
        E::Memory(FrameError::InvalidVirtualAddress | FrameError::InvalidPageFlags) => {
            carlio_os::linux_abi::EINVAL
        }
        E::Memory(_) | E::RollbackFailed => carlio_os::linux_abi::EIO,
    }
}

#[cfg(target_os = "none")]
fn network_errno(error: carlio_os::net::NetworkError) -> u16 {
    use carlio_os::net::NetworkError as N;
    match error {
        N::AddressInUse => carlio_os::linux_abi::EADDRINUSE,
        N::PermissionDenied => carlio_os::linux_abi::EACCES,
        N::BadDescriptor => carlio_os::linux_abi::EBADF,
        N::UnsupportedSocket => carlio_os::linux_abi::EAFNOSUPPORT,
        N::InvalidPort | N::InvalidState => carlio_os::linux_abi::EINVAL,
        _ => carlio_os::linux_abi::ENFILE,
    }
}

#[cfg(target_os = "none")]
fn vfs_errno(error: carlio_os::vfs::VfsError) -> u16 {
    use carlio_os::vfs::VfsError as V;
    match error {
        V::NotFound => carlio_os::linux_abi::ENOENT,
        V::PermissionDenied | V::NotOwner | V::MountDenied => carlio_os::linux_abi::EACCES,
        V::IsDirectory => carlio_os::linux_abi::EISDIR,
        V::FileTooLarge => carlio_os::linux_abi::EFBIG,
        V::Full | V::MountTableFull => carlio_os::linux_abi::ENFILE,
        V::InvalidPath | V::NotDirectory | V::BufferTooSmall => carlio_os::linux_abi::EINVAL,
        _ => carlio_os::linux_abi::EIO,
    }
}

#[cfg(target_os = "none")]
pub(crate) fn is_secondary_cpu(cpu: usize) -> bool {
    BOOT_CPU_LOCALS.get(cpu).is_some_and(|local| {
        local.lapic_id.load(Ordering::Acquire) != SMP_BSP_LAPIC_ID.load(Ordering::Acquire)
    })
}

#[cfg(target_os = "none")]
pub(crate) fn process_probe_result() -> u64 {
    let mut ap_probe_passed = false;
    let ap_mask = secondary_cpu_mask();
    for _ in 0..200 {
        let active = AP_CPL3_CPU_MASK.load(Ordering::Acquire);
        let idle = AP_CPL3_IDLE_MASK.load(Ordering::Acquire);
        if AP_CPL3_OVERLAP_PASSED.load(Ordering::Acquire)
            && active.count_ones() >= 2
            && idle & ap_mask == ap_mask
            && AP_CPL3_RUNNING_MASK.load(Ordering::Acquire) & ap_mask == 0
        {
            ap_probe_passed = true;
            break;
        }
        if !idt::wait_ticks(1) {
            break;
        }
    }
    if !ap_probe_passed {
        AP_CPL3_STOP.store(true, Ordering::Release);
        write("SMP CPL3: gate falhou; APs simultâneos ou retorno a idle ausentes.\r\n");
        for _ in 0..200 {
            if AP_CPL3_IDLE_MASK.load(Ordering::Acquire) & ap_mask == ap_mask
                && AP_CPL3_RUNNING_MASK.load(Ordering::Acquire) & ap_mask == 0
            {
                break;
            }
            if !idt::wait_ticks(1) {
                break;
            }
        }
    }
    PROCESS_PROBE_DONE.store(true, Ordering::Release);
    let switches = PROCESS_PROBE_SWITCHES.load(Ordering::Acquire);
    let cr3_switches = PROCESS_PROBE_CR3_SWITCHES.load(Ordering::Acquire);
    let faults = PROCESS_PROBE_FAULTS.load(Ordering::Acquire);
    write("Runtime: tarefas encerradas; trocas de timer=");
    write_u64(switches);
    write(", trocas de CR3=");
    write_u64(cr3_switches);
    write(", faults isolados=");
    write_u64(faults as u64);
    write(".\r\n");
    if ap_probe_passed {
        write("SMP CPL3: sobreposição simultânea confirmada em dois APs; ambos retornaram a idle.\r\n");
    }
    if LINUX_GETPID_PROBE_PASSED.load(Ordering::Acquire) {
        write("ABI Linux x86_64: getpid/exit via SYSCALL passou.\r\n");
    } else {
        write("ABI Linux x86_64: chamada CPL3 não passou.\r\n");
    }
    let abi_probe_mask = LINUX_CREDENTIAL_PROBE_MASK.load(Ordering::Acquire);
    if abi_probe_mask & 0x1f == 0x1f {
        write("ABI Linux: getuid/geteuid/getgid/getegid/getgroups via SYSCALL passaram.\r\n");
    } else {
        write("ABI Linux: probe CPL3 de credenciais incompleto; máscara=");
        write_hex_u64(abi_probe_mask as u64);
        write(".\r\n");
    }
    if abi_probe_mask & 0x20 != 0 {
        write("ABI Linux: write CPL3 validou buffer de usuário e serial.\r\n");
    } else {
        write("ABI Linux: probe CPL3 de write não passou.\r\n");
    }
    if abi_probe_mask & 0x40 != 0 {
        write("ABI Linux: read CPL3 recusou destino inválido antes da leitura serial.\r\n");
    } else {
        write("ABI Linux: probe CPL3 de read não passou.\r\n");
    }
    if abi_probe_mask & 0x80 != 0 {
        write("ABI Linux: openat/read/close CPL3 leu arquivo pelo VFS.\r\n");
    } else {
        write("ABI Linux: leitura de arquivo CPL3 pelo VFS não passou.\r\n");
    }
    if LINUX_VFS_DENIAL_PROBE_PASSED.load(Ordering::Acquire) {
        write("Segurança CPL3: openat negou arquivo 0600 de outro UID com EACCES.\r\n");
    } else {
        write("Segurança CPL3: negação POSIX de openat não passou.\r\n");
        write("ABI diagnóstico: retorno de openat negado=");
        write_hex_u64(LINUX_VFS_DENIAL_RESULT.load(Ordering::Acquire));
        write(".\r\n");
    }
    if abi_probe_mask & 0x100 != 0 {
        write("ABI Linux: read(0) CPL3 recebeu entrada serial.\r\n");
    } else {
        write("ABI Linux: read(0) CPL3 não recebeu entrada serial.\r\n");
    }
    let memory_probe_mask = LINUX_MEMORY_PROBE_MASK.load(Ordering::Acquire);
    if memory_probe_mask & 0x3 == 0x3 {
        write("ABI Linux: mmap/munmap CPL3 alocou, gravou e liberou páginas do processo.\r\n");
    } else {
        write("ABI Linux: probe CPL3 de mmap/munmap incompleto; máscara=");
        write_hex_u64(memory_probe_mask as u64);
        write(", munmap=");
        write_hex_u64(LINUX_MUNMAP_RESULT.load(Ordering::Acquire));
        write(".\r\n");
    }
    if switches > 0
        && ap_probe_passed
        && LINUX_GETPID_PROBE_PASSED.load(Ordering::Acquire)
        && abi_probe_mask & 0x1ff == 0x1ff
        && memory_probe_mask & 0x3 == 0x3
    {
        0xfff
    } else {
        0x7ff
    }
}

#[cfg(target_os = "none")]
pub(crate) fn process_probe_is_active() -> bool {
    PROCESS_PROBE_STARTED.load(Ordering::Acquire)
}

#[cfg(target_os = "none")]
pub(crate) fn process_probe_is_done() -> bool {
    PROCESS_PROBE_DONE.load(Ordering::Acquire)
}

#[cfg(target_os = "none")]
fn reap_process_probe() -> bool {
    let mut success = true;
    for index in 0..3 {
        let id = PROCESS_PROBE_IDS[index].load(Ordering::Acquire);
        if id == 0 {
            continue;
        }
        let result = PROCESS_SCHEDULER.reap_with(id, &PROCESS_KERNEL_STACKS, |process| {
            let storage = if process.id == PROCESS_PROBE_IDS[0].load(Ordering::Acquire) {
                &PROCESS_PROBE_SPACE_A
            } else if process.id == PROCESS_PROBE_IDS[1].load(Ordering::Acquire) {
                &PROCESS_PROBE_SPACE_B
            } else if process.id == PROCESS_PROBE_IDS[2].load(Ordering::Acquire) {
                &PROCESS_PROBE_SPACE_C
            } else {
                return Err(());
            };
            if destroy_process_probe_space(storage) {
                Ok(())
            } else {
                Err(())
            }
        });
        if result.is_ok() {
            PROCESS_PROBE_IDS[index].store(0, Ordering::Release);
        } else {
            write("Reaper: cleanup do processo falhou; recursos preservados para retry.\r\n");
            success = false;
        }
    }
    success
}

#[cfg(target_os = "none")]
fn quiesce_process_probe_aps() -> bool {
    let kernel_root = PROCESS_KERNEL_CR3.load(Ordering::Acquire);
    let bsp_root: u64;
    // SAFETY: o BSP lê seu CR3 atual enquanto executa código e stack do kernel.
    unsafe { asm!("mov {}, cr3", out(reg) bsp_root, options(nomem, nostack, preserves_flags)) };
    if kernel_root == 0 || bsp_root & 0x000f_ffff_ffff_f000 != kernel_root {
        write("Reaper SMP: BSP não confirmou CR3 kernel; liberação cancelada.\r\n");
        return false;
    }
    // APs ainda marcados em CPL3 não causam cancelamento imediato: o epoch
    // abaixo os mantém no scheduler até cada um trocar stack/CR3 e confirmar.
    let Some(epoch) = PROCESS_REAP_BARRIER.begin() else {
        return false;
    };
    let required_aps = match u64::try_from(secondary_cpu_mask()) {
        Ok(mask) => mask,
        Err(_) => {
            let _ = PROCESS_REAP_BARRIER.release(epoch);
            return false;
        }
    };
    for _ in 0..200 {
        let all_quiescent = PROCESS_REAP_BARRIER.complete(epoch, required_aps)
            && AP_CPL3_RUNNING_MASK.load(Ordering::Acquire) & secondary_cpu_mask() == 0;
        if all_quiescent {
            let acknowledgements = required_aps.count_ones();
            write("Reaper SMP: BSP e APs confirmaram CR3 kernel; ACKs dos APs=");
            write_u64(u64::from(acknowledgements));
            write("; CPL3 ativo=0; stacks, VM e page tables liberáveis.\r\n");
            return true;
        }
        if !idt::wait_ticks(1) {
            let _ = PROCESS_REAP_BARRIER.release(epoch);
            return false;
        }
    }
    let _ = PROCESS_REAP_BARRIER.release(epoch);
    false
}

#[cfg(target_os = "none")]
fn release_process_probe_aps() {
    if let Some(epoch) = PROCESS_REAP_BARRIER.requested() {
        let _ = PROCESS_REAP_BARRIER.release(epoch);
    }
}

#[cfg(target_os = "none")]
fn destroy_process_probe_space(storage: &IrqSpinLock<Option<RuntimeProcessVm>>) -> bool {
    let Some(space) = storage.lock().take() else {
        return false;
    };
    match space.try_destroy() {
        Ok(_access) => true,
        Err((space, _error)) => {
            write("Reaper: teardown do espaço de usuário recusado.\r\n");
            *storage.lock() = Some(space);
            false
        }
    }
}

#[cfg(target_os = "none")]
pub(crate) fn ring3_kernel_routine_selftest() -> u64 {
    use carlio_os::{
        elf::{ElfError, ElfImage},
        process::{ProcessState, Scheduler},
        user_memory::{self, UserBufferError},
    };

    const ELF_ADDRESS: u64 = 0x0000_4000_0000_0100;
    const STACK_ADDRESS: u64 = 0x0000_4000_0000_1ff0;
    const UNMAPPED_ADDRESS: u64 = 0x0000_4000_0000_2000;
    const ELF_LENGTH: usize = 120;
    let mut results = 0u64;
    let mut set = |bit: u32, passed: bool| {
        if passed {
            results |= 1 << bit;
        }
    };

    let hhdm = unsafe { core::ptr::addr_of!(HHDM_OFFSET).read_volatile() };
    let mut root: u64;
    unsafe { asm!("mov {}, cr3", out(reg) root, options(nomem, nostack, preserves_flags)) };
    root &= 0x000f_ffff_ffff_f000;
    let Ok(lookup) = PageTableManager::new(BootPageTableAccess { hhdm }, root) else {
        return 0;
    };

    let mut elf_bytes = [0u8; ELF_LENGTH];
    let copied = unsafe { user_memory::copy_from_user(&lookup, ELF_ADDRESS, &mut elf_bytes) };
    set(0, copied.is_ok());
    set(1, ElfImage::<4>::parse(&elf_bytes).is_ok());
    let mut malformed = elf_bytes;
    malformed[68..72].copy_from_slice(&7u32.to_le_bytes());
    set(
        2,
        ElfImage::<4>::parse(&malformed) == Err(ElfError::InvalidSegment),
    );
    set(
        3,
        user_memory::validate(&lookup, ELF_ADDRESS, ELF_LENGTH, false).is_ok(),
    );
    set(
        4,
        user_memory::validate(&lookup, ELF_ADDRESS, 1, true) == Err(UserBufferError::ReadOnly),
    );
    set(
        5,
        user_memory::validate(&lookup, UNMAPPED_ADDRESS, 1, false)
            == Err(UserBufferError::Unmapped),
    );
    set(
        6,
        user_memory::validate(&lookup, u64::MAX - 2, 8, false)
            == Err(UserBufferError::AddressOverflow),
    );
    let payload = [0x43, 0x41, 0x52, 0x4c, 0x49, 0x4f];
    let write_ok = unsafe { user_memory::copy_to_user(&lookup, STACK_ADDRESS, &payload) };
    let mut returned = [0u8; 6];
    let read_back = unsafe { user_memory::copy_from_user(&lookup, STACK_ADDRESS, &mut returned) };
    set(
        7,
        write_ok.is_ok() && read_back.is_ok() && returned == payload,
    );
    set(
        8,
        unsafe { user_memory::copy_to_user(&lookup, ELF_ADDRESS, &payload) }
            == Err(UserBufferError::ReadOnly),
    );
    set(
        9,
        unsafe { user_memory::copy_from_user(&lookup, UNMAPPED_ADDRESS, &mut returned) }
            == Err(UserBufferError::Unmapped),
    );

    let scheduler = Scheduler::<4, 2>::new();
    let first = scheduler.create(root, ELF_ADDRESS, STACK_ADDRESS).ok();
    let second = scheduler.create(root, ELF_ADDRESS, STACK_ADDRESS).ok();
    let first_id = first.unwrap_or(0);
    let second_id = second.unwrap_or(0);
    let schedule0 = scheduler.schedule(0).ok().flatten();
    let schedule1 = scheduler.schedule(1).ok().flatten();
    let rotate0 = scheduler.schedule(0).ok().flatten();
    let block = scheduler.block(0).is_ok();
    let wake = scheduler.wake(first_id).is_ok();
    let resume0 = scheduler.schedule(0).ok().flatten();
    let exit = scheduler.exit(0, 23).is_ok();
    let exited =
        scheduler.process(first_id).map(|p| p.state) == Some(ProcessState::Exited { code: 23 });
    let invalid_cpu = scheduler.schedule(2).is_err();
    set(
        10,
        first_id != 0
            && second_id != 0
            && schedule0.map(|p| p.id) == Some(first_id)
            && schedule1.map(|p| p.id) == Some(second_id)
            && rotate0.map(|p| p.id) == Some(first_id)
            && block
            && wake
            && resume0.map(|p| p.id) == Some(first_id)
            && exit
            && exited
            && invalid_cpu,
    );
    drop(set);
    results
}

#[cfg(target_os = "none")]
fn prepare_ring3_elf_fixture(bytes: &mut [u8; 120]) {
    bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
    bytes[18..20].copy_from_slice(&62u16.to_le_bytes());
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[24..32].copy_from_slice(&0x0040_0000u64.to_le_bytes());
    bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
    bytes[52..54].copy_from_slice(&64u16.to_le_bytes());
    bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
    bytes[56..58].copy_from_slice(&1u16.to_le_bytes());
    bytes[64..68].copy_from_slice(&1u32.to_le_bytes());
    bytes[68..72].copy_from_slice(&5u32.to_le_bytes());
    bytes[72..80].copy_from_slice(&120u64.to_le_bytes());
    bytes[80..88].copy_from_slice(&0x0040_0000u64.to_le_bytes());
    bytes[104..112].copy_from_slice(&16u64.to_le_bytes());
}

#[cfg(target_os = "none")]
fn record_error(error: KernelError) {
    // O kernel ainda executa somente na CPU de boot; o logger será protegido
    // por lock/atomics quando SMP e interrupções forem habilitados.
    unsafe {
        core::ptr::addr_of_mut!(ERROR_LOG)
            .as_mut()
            .unwrap()
            .record(error);
    }
}

#[cfg(target_os = "none")]
fn report_errors() {
    let log = unsafe { core::ptr::addr_of!(ERROR_LOG).read() };
    write("Erros registrados: ");
    write_u32(log.total());
    write("\r\n");
    for index in 0..log.len() {
        if let Some(error) = log.recent(index) {
            write("- ");
            write(error.message());
            write("\r\n");
        }
    }
}

#[cfg(target_os = "none")]
fn report_errors_markdown() {
    let log = unsafe { core::ptr::addr_of!(ERROR_LOG).read() };
    write("# Eventos do kernel (buffer volátil)\r\n\r\n");
    write("| Métrica | Valor | Gráfico |\r\n|---|---:|---|\r\n");
    write("| Eventos desde o boot | ");
    write_u32(log.total());
    write(" | ");
    let bars = log.total().min(24);
    for _ in 0..bars {
        write("#");
    }
    write(" |\r\n\r\n| Índice recente | Código | Evento |\r\n|---:|---:|---|\r\n");
    for index in 0..log.len() {
        if let Some(error) = log.recent(index) {
            write("| ");
            write_u32(index as u32);
            write(" | ");
            write_u32(error as u16 as u32);
            write(" | ");
            write(error.message());
            write(" |\r\n");
        }
    }
    write("\r\nAviso: captura em RAM, limitada e perdida ao reiniciar; não é log persistente.\r\n");
}

#[cfg(target_os = "none")]
fn report_memory_map() {
    let hhdm = unsafe { core::ptr::addr_of!(HHDM_REQUEST.response).read_volatile() };
    let map = unsafe { core::ptr::addr_of!(MEMMAP_REQUEST.response).read_volatile() };
    let kernel = unsafe { core::ptr::addr_of!(KERNEL_ADDRESS_REQUEST.response).read_volatile() };
    if hhdm.is_null() || map.is_null() || kernel.is_null() {
        record_error(KernelError::BootInfoMissing);
        write(
            "Aviso: mapa/HHDM/endereço do kernel indisponível; drivers DMA/MMIO desativados.\r\n",
        );
        return;
    }
    let offset = unsafe { (*hhdm).offset };
    let entry_count = unsafe { (*map).entry_count.min(4096) };
    let entries = unsafe { (*map).entries };
    if entries.is_null() {
        record_error(KernelError::BootInfoMissing);
        write("Aviso: lista do mapa de memória inválida; drivers DMA/MMIO desativados.\r\n");
        return;
    }
    let mut usable_bytes = 0u64;
    let mut usable_regions = 0u32;
    for index in 0..entry_count as usize {
        let entry = unsafe { entries.add(index).read_volatile() };
        if entry.is_null() {
            continue;
        }
        let entry = unsafe { &*entry };
        if entry.kind == 0 {
            usable_regions = usable_regions.saturating_add(1);
            usable_bytes = usable_bytes.saturating_add(entry.length);
            if FRAME_ALLOCATOR
                .lock()
                .add_usable_region(entry.base, entry.length)
                .is_err()
            {
                record_error(KernelError::BootInfoMissing);
            }
        }
    }
    unsafe {
        core::ptr::addr_of_mut!(HHDM_OFFSET).write_volatile(offset);
        core::ptr::addr_of_mut!(KERNEL_PHYSICAL_BASE).write_volatile((*kernel).physical_base);
        core::ptr::addr_of_mut!(KERNEL_VIRTUAL_BASE).write_volatile((*kernel).virtual_base);
    }
    write("Mapa de memória Limine: regiões usáveis=");
    write_u32(usable_regions);
    write(", RAM usável MiB=");
    write_u32((usable_bytes / (1024 * 1024)).min(u32::MAX as u64) as u32);
    write("; HHDM e bases físico/virtual do kernel disponíveis.\r\n");
    write("Allocator físico: frames livres até 32 GiB=");
    write_u64(FRAME_ALLOCATOR.lock().free_frames() as u64);
    let high_memory_probe = FRAME_ALLOCATOR
        .lock()
        .allocate_at_or_above(4 * 1024 * 1024 * 1024);
    let probe_frame = high_memory_probe.or_else(|| FRAME_ALLOCATOR.lock().allocate());
    if high_memory_probe.is_some() {
        write(", frame acima de 4 GiB alocado e testado");
    } else {
        write(", sem RAM acima de 4 GiB; teste usa frame baixo");
    }
    let probe_ok = if let Some(physical) = probe_frame {
        let probe_address = offset.checked_add(physical);
        if let Some(virtual_address) = probe_address {
            let pointer = virtual_address as *mut u64;
            unsafe {
                core::ptr::write_volatile(pointer, 0x4341_524c_494f_4f53);
                let valid = core::ptr::read_volatile(pointer) == 0x4341_524c_494f_4f53;
                core::ptr::write_volatile(pointer, 0);
                valid
            }
        } else {
            false
        }
    } else {
        false
    };
    if let Some(physical) = probe_frame {
        let _ = FRAME_ALLOCATOR.lock().free(physical);
    }
    if probe_ok {
        write("; alocação, escrita, leitura e liberação passaram.\r\n");
    } else {
        record_error(KernelError::DeviceInitFailed);
        write("; teste de acesso ao frame falhou.\r\n");
    }
    if probe_ok {
        PAGE_TABLE_PROBE_PENDING.store(true, Ordering::Release);
    }
}

#[cfg(target_os = "none")]
fn page_table_boot_probe(hhdm: u64) {
    const TEST_VIRTUAL: u64 = 0xffff_a000_0000_0000;
    const TEST_USER_VIRTUAL: u64 = 0x0000_4000_0000_0000;
    let frame = FRAME_ALLOCATOR.lock().allocate();
    let Some(physical) = frame else {
        write("VM: sem frame para o teste de mapeamento.\r\n");
        return;
    };
    let root: u64;
    unsafe { asm!("mov {}, cr3", out(reg) root, options(nomem, nostack, preserves_flags)) };
    let root = root & 0x000f_ffff_ffff_f000;
    let access = BootPageTableAccess { hhdm };
    let mut tables = match PageTableManager::new(access, root) {
        Ok(manager) => manager,
        Err(_) => {
            let _ = FRAME_ALLOCATOR.lock().free(physical);
            write("VM: CR3 inválido; teste de paginação não executado.\r\n");
            return;
        }
    };
    let result = tables.map(TEST_VIRTUAL, physical, 0x3).and_then(|()| {
        let pointer = TEST_VIRTUAL as *mut u64;
        unsafe { core::ptr::write_volatile(pointer, 0x5041_4745_5441_424c) };
        let valid = unsafe { core::ptr::read_volatile(pointer) == 0x5041_4745_5441_424c };
        let unmapped = tables.unmap(TEST_VIRTUAL)? == physical;
        if !valid || !unmapped {
            return Err(FrameError::CorruptTable);
        }
        tables.map_user(TEST_USER_VIRTUAL, physical, 0x3)?;
        let user_mapping = tables.mapping(TEST_USER_VIRTUAL)?;
        let user_pointer = TEST_USER_VIRTUAL as *mut u64;
        unsafe { core::ptr::write_volatile(user_pointer, 0x5553_4552_5041_4745) };
        let user_access_valid =
            unsafe { core::ptr::read_volatile(user_pointer) == 0x5553_4552_5041_4745 }
                && user_mapping.flags & PAGE_FLAG_USER != 0;
        let user_unmapped = tables.unmap(TEST_USER_VIRTUAL)? == physical;
        if user_access_valid && user_unmapped {
            Ok(())
        } else {
            Err(FrameError::CorruptTable)
        }
    });
    let address_space_result = if result.is_ok() {
        (|| {
            const USER_STACK_VIRTUAL: u64 = TEST_USER_VIRTUAL + 4096;
            const USER_STACK_TOP: u64 = USER_STACK_VIRTUAL + 4096;
            const USER_PROBE_CODE: [u8; 7] = [
                0xb8, 0xed, 0xfe, 0x00, 0x00, // mov eax, SYS_KERNEL_ROUTINE_PROBE
                0xcd, 0x80, // int 0x80; kernel executa a bateria de prova
            ];
            let stack_physical = FRAME_ALLOCATOR
                .lock()
                .allocate()
                .ok_or(FrameError::TableAllocationFailed)?;
            let code_pointer =
                hhdm.checked_add(physical)
                    .ok_or(FrameError::InvalidPhysicalAddress)? as *mut u8;
            let stack_pointer =
                hhdm.checked_add(stack_physical)
                    .ok_or(FrameError::InvalidPhysicalAddress)? as *mut u8;
            unsafe {
                core::ptr::write_bytes(code_pointer, 0, 4096);
                core::ptr::copy_nonoverlapping(
                    USER_PROBE_CODE.as_ptr(),
                    code_pointer,
                    USER_PROBE_CODE.len(),
                );
                let mut elf_fixture = [0u8; 120];
                prepare_ring3_elf_fixture(&mut elf_fixture);
                core::ptr::copy_nonoverlapping(
                    elf_fixture.as_ptr(),
                    code_pointer.add(0x100),
                    elf_fixture.len(),
                );
                core::ptr::write_bytes(stack_pointer, 0, 4096);
            }

            let mut space = match OwnedAddressSpace::<_, 2>::new(BootPageTableAccess { hhdm }, root)
            {
                Ok(space) => space,
                Err(error) => {
                    let _ = FRAME_ALLOCATOR.lock().free(stack_physical);
                    return Err(error);
                }
            };
            let mut code_transferred = false;
            let mut stack_transferred = false;
            let execution = (|| {
                space.map_owned_frame(TEST_USER_VIRTUAL, physical, 0x1)?;
                code_transferred = true;
                space.map_owned_frame(USER_STACK_VIRTUAL, stack_physical, 0x3)?;
                stack_transferred = true;
                let code_mapping = space.mapping(TEST_USER_VIRTUAL)?;
                let stack_mapping = space.mapping(USER_STACK_VIRTUAL)?;
                let kernel_cannot_see_user_va =
                    PageTableManager::new(BootPageTableAccess { hhdm }, root)?
                        .mapping(TEST_USER_VIRTUAL)
                        == Err(FrameError::NotMapped);
                let old_cr3: u64;
                unsafe {
                    asm!("mov {}, cr3", out(reg) old_cr3, options(nomem, nostack, preserves_flags))
                };
                PROCESS_KERNEL_CR3.store(old_cr3 & 0x000f_ffff_ffff_f000, Ordering::Release);
                let process_cr3 = space.root();
                unsafe {
                    asm!("mov cr3, {}", in(reg) process_cr3, options(nostack, preserves_flags))
                };
                unsafe { idt::start_watchdog() };
                let routine_results =
                    unsafe { idt::run_ring3_probe(TEST_USER_VIRTUAL, USER_STACK_TOP) };
                unsafe {
                    asm!("mov cr3, {}", in(reg) process_cr3, options(nostack, preserves_flags))
                };
                unsafe { idt::restore_bootstrap_kernel_stack() };
                let entered = routine_results & 0x7ff == 0x7ff;
                unsafe { asm!("mov cr3, {}", in(reg) old_cr3, options(nostack, preserves_flags)) };
                let quiescent = quiesce_process_probe_aps();
                if quiescent {
                    PROCESS_PROBE_STARTED.store(false, Ordering::Release);
                }
                let reaped = quiescent && reap_process_probe();
                let preempted = routine_results == 0xfff && reaped;
                if quiescent {
                    release_process_probe_aps();
                }
                unsafe {
                    asm!("mov cr3, {}", in(reg) process_cr3, options(nostack, preserves_flags))
                };
                unsafe {
                    core::ptr::write_volatile(code_pointer, 0x0f);
                    core::ptr::write_volatile(code_pointer.add(1), 0x0b);
                }
                let fault_result =
                    unsafe { idt::run_ring3_probe(TEST_USER_VIRTUAL, USER_STACK_TOP) };
                let fault_recovered = fault_result == (0x100 | 6);
                unsafe { asm!("mov cr3, {}", in(reg) old_cr3, options(nostack, preserves_flags)) };
                if entered
                    && preempted
                    && fault_recovered
                    && kernel_cannot_see_user_va
                    && code_mapping.physical == physical
                    && code_mapping.flags & PAGE_FLAG_USER != 0
                    && stack_mapping.physical == stack_physical
                    && stack_mapping.flags & PAGE_FLAG_USER != 0
                    && stack_mapping.flags & 2 != 0
                {
                    Ok(())
                } else {
                    write("VM debug: entered=");
                    write(if entered { "1" } else { "0" });
                    write(" preempted=");
                    write(if preempted { "1" } else { "0" });
                    write(" fault=");
                    write(if fault_recovered { "1" } else { "0" });
                    write(" kernel-isolated=");
                    write(if kernel_cannot_see_user_va { "1" } else { "0" });
                    write(" fault-result=");
                    write_hex_u64(fault_result);
                    write(".\r\n");
                    Err(FrameError::CorruptTable)
                }
            })();
            let destroyed = match space.try_destroy() {
                Ok(_access) => Ok(()),
                Err((_space, error)) => Err(error),
            };
            if !stack_transferred {
                let _ = FRAME_ALLOCATOR.lock().free(stack_physical);
            }
            if !code_transferred {
                let _ = FRAME_ALLOCATOR.lock().free(physical);
            }
            execution.and(destroyed)
        })()
    } else {
        Err(FrameError::CorruptTable)
    };
    if address_space_result.is_err() {
        // O frame pode ter sido liberado pelo espaço com ownership; o allocator
        // recusa double-free e mantém essa verificação como última proteção.
        let _ = FRAME_ALLOCATOR.lock().free(physical);
    }
    if result.is_ok() && address_space_result.is_ok() {
        write("Ring 3: probe CPL3, retorno seguro e fault #UD isolado passaram.\r\n");
        write("Ring 3 rotinas: ELF carregado em CPL3, scheduler e preempção entre dois CR3 passaram; pilhas recuperadas pelo reaper.\r\n");
        write("TLB SMP: shootdowns enviados=");
        write_u32(lapic::shootdown_count().min(u32::MAX as usize) as u32);
        write(", ACKs remotos por IPI concluídos antes de liberar tabelas.\r\n");
    } else {
        write("VM: teste de tabelas de página falhou; kernel manteve CR3 do Limine.\r\n");
        record_error(KernelError::DeviceInitFailed);
    }
}

#[cfg(target_os = "none")]
fn kernel_heap_boot_probe() {
    use core::alloc::Layout;
    use core::ptr::NonNull;

    let region = core::ptr::addr_of_mut!(KERNEL_HEAP_BOOT_REGION).cast::<u8>();
    {
        let mut global = KERNEL_HEAP.lock();
        if global.region_count() != 0
            || unsafe { global.add_region(NonNull::new(region).unwrap(), 64 * 1024) }.is_err()
        {
            record_error(KernelError::DeviceInitFailed);
            write("Heap kernel: inicialização da região de bootstrap falhou.\r\n");
            return;
        }
    }
    let layout = Layout::from_size_align(257, 64).unwrap();
    let Some(allocation) = kernel_heap_allocate(layout) else {
        record_error(KernelError::DeviceInitFailed);
        write("Heap kernel: alocação de prova falhou.\r\n");
        return;
    };
    unsafe {
        allocation.as_ptr().write_bytes(0xA7, layout.size());
    }
    let valid =
        unsafe { (0..layout.size()).all(|offset| allocation.as_ptr().add(offset).read() == 0xA7) };
    let released = unsafe { kernel_heap_deallocate(allocation).is_ok() };
    let mut allocations = [None; 128];
    let stress_layout = Layout::from_size_align(600, 16).unwrap();
    for item in &mut allocations {
        *item = kernel_heap_allocate(stress_layout);
    }
    let grew = KERNEL_HEAP.lock().region_count() > 1;
    let stress_ok = allocations.iter().all(Option::is_some);
    for allocation in allocations.into_iter().flatten() {
        let _ = unsafe { kernel_heap_deallocate(allocation) };
    }
    if valid && released && stress_ok && grew {
        write("Heap kernel: regiões dinâmicas, alinhamento, escrita/leitura, expansão e liberação passaram.\r\n");
    } else {
        record_error(KernelError::DeviceInitFailed);
        write("Heap kernel: teste de alocação/liberação falhou.\r\n");
    }
}

#[cfg(target_os = "none")]
fn kernel_heap_allocate(layout: core::alloc::Layout) -> Option<core::ptr::NonNull<u8>> {
    if let Some(pointer) = KERNEL_HEAP.lock().allocate(layout) {
        return Some(pointer);
    }
    // Uma região adicionada pelo crescimento tem exatamente um frame.
    if layout.size() > 4096 {
        return None;
    }
    // Nunca manter locks de heap e allocator de frames simultaneamente.
    let physical = FRAME_ALLOCATOR.lock().allocate()?;
    let hhdm = unsafe { core::ptr::addr_of!(HHDM_OFFSET).read_volatile() };
    let virtual_address = match hhdm.checked_add(physical) {
        Some(address) => address,
        None => {
            let _ = FRAME_ALLOCATOR.lock().free(physical);
            return None;
        }
    };
    let Some(region) = core::ptr::NonNull::new(virtual_address as *mut u8) else {
        let _ = FRAME_ALLOCATOR.lock().free(physical);
        return None;
    };
    unsafe { core::ptr::write_bytes(region.as_ptr(), 0, 4096) };
    let mut heap = KERNEL_HEAP.lock();
    if unsafe { heap.add_region(region, 4096) }.is_err() {
        drop(heap);
        let _ = FRAME_ALLOCATOR.lock().free(physical);
        return None;
    }
    heap.allocate(layout)
}

#[cfg(target_os = "none")]
unsafe fn kernel_heap_deallocate(
    pointer: core::ptr::NonNull<u8>,
) -> Result<(), carlio_os::heap::HeapError> {
    KERNEL_HEAP.lock().deallocate(pointer)
}

#[cfg(target_os = "none")]
fn write_u32(mut number: u32) {
    let mut digits = [0u8; 10];
    let mut len = 0;
    loop {
        digits[len] = b'0' + (number % 10) as u8;
        len += 1;
        number /= 10;
        if number == 0 {
            break;
        }
    }
    while len > 0 {
        len -= 1;
        if !serial_write(digits[len]) {
            return;
        }
    }
}

#[cfg(target_os = "none")]
fn write_hex_u64(value: u64) {
    for shift in (0..16).rev() {
        write_hex_nibble(((value >> (shift * 4)) & 0xf) as u8);
    }
}

#[cfg(target_os = "none")]
fn write_u64(value: u64) {
    let mut digits = [0u8; 20];
    let mut len = 0;
    let mut number = value;
    loop {
        digits[len] = b'0' + (number % 10) as u8;
        len += 1;
        number /= 10;
        if number == 0 {
            break;
        }
    }
    while len > 0 {
        len -= 1;
        if !serial_write(digits[len]) {
            return;
        }
    }
}

#[cfg(target_os = "none")]
fn enumerate_pci() {
    write("Inventário PCI (somente leitura):\r\n");
    let found = pci_device::enumerate(report_pci_function);
    write("Controladores relevantes encontrados: ");
    write_u32(found as u32);
    write("\r\n");
}

#[cfg(target_os = "none")]
fn initialize_persistent_rootfs() {
    write("Hermione rootfs: verificando volume de sistema.\r\n");
    let Some(device) = AHCI_BLOCK_DEVICE.lock().take() else {
        write("Hermione rootfs: AHCI sem dispositivo; mantendo VFS somente em RAM.\r\n");
        return;
    };
    let mut volume = match carlio_os::hermione::HermioneVolume::mount(device) {
        Ok(volume) => volume,
        Err(_) => {
            write("Hermione rootfs: volume ausente ou inválido; disco preservado, sem formatação automática.\r\n");
            return;
        }
    };
    write("Hermione rootfs: volume montado; lendo snapshot.\r\n");
    let mut directory = [None; carlio_os::hermione::MAX_FILES];
    if volume.list(&mut directory).is_err() {
        write("Hermione rootfs: falha ao validar diretório do volume.\r\n");
        return;
    }
    write("Hermione rootfs: diretório validado.\r\n");
    let mut snapshot = ROOTFS_SNAPSHOT.lock();
    match volume.read_file_with_progress("rootfs.img", &mut snapshot[..], |stage| {
        write("Hermione rootfs: fase de leitura ");
        write_u32(stage as u32);
        write(".\r\n");
    }) {
        Ok(length) => {
            let mut staging = ROOTFS_VFS_STAGING.lock();
            match staging.restore_snapshot(&snapshot[..length]) {
                Ok(()) => {
                    core::mem::swap(&mut *PROCESS_VFS.lock(), &mut *staging);
                    write("Hermione rootfs persistente montado para syscalls Linux.\r\n");
                }
                Err(_) => {
                    write("Hermione rootfs: snapshot corrompido; não foi substituído.\r\n");
                    return;
                }
            }
        }
        Err(carlio_os::hermione::FsError::FileNotFound) => {
            let mut vfs = PROCESS_VFS.lock();
            if vfs.create_fhs_layout().is_err()
                || vfs
                    .create_dir("/", "programas", Credentials { uid: 0, gid: 0 }, 0o755)
                    .is_err()
            {
                write("Hermione rootfs: falha ao preparar a árvore inicial.\r\n");
                return;
            }
            let length = match vfs.encode_snapshot(&mut snapshot[..]) {
                Ok(length) => length,
                Err(_) => {
                    write("Hermione rootfs: árvore inicial excede o limite de snapshot.\r\n");
                    return;
                }
            };
            drop(vfs);
            if volume
                .write_file("rootfs.img", &snapshot[..length])
                .is_err()
            {
                write("Hermione rootfs: não foi possível gravar o snapshot inicial.\r\n");
                return;
            }
            write("Hermione rootfs inicial persistido; limite atual por arquivo: 16 KiB.\r\n");
        }
        Err(_) => {
            write("Hermione rootfs: erro de leitura; mídia preservada.\r\n");
            return;
        }
    }
    *PERSISTENT_ROOTFS.lock() = Some(volume);
}

#[cfg(target_os = "none")]
fn persist_linux_rootfs() -> Result<(), u16> {
    let mut snapshot = ROOTFS_SNAPSHOT.lock();
    let length = PROCESS_VFS
        .lock()
        .encode_snapshot(&mut snapshot[..])
        .map_err(|_| carlio_os::linux_abi::EIO)?;
    let mut rootfs = PERSISTENT_ROOTFS.lock();
    if let Some(volume) = rootfs.as_mut() {
        volume
            .write_file("rootfs.img", &snapshot[..length])
            .map_err(|_| carlio_os::linux_abi::EIO)?;
    }
    Ok(())
}

#[cfg(target_os = "none")]
fn report_pci_function(function: PciFunction) {
    write("PCI ");
    write_hex_byte(function.bus);
    write(":");
    write_hex_byte(function.device);
    write(".");
    write_hex_nibble(function.function);
    write(" ");
    write_hex_word(function.vendor_id);
    write(":");
    write_hex_word(function.device_id);
    if function.is_sata_ahci() {
        write(" SATA/AHCI");
        match function.ahci_model() {
            Some(carlio_os::pci::AhciModel::IntelRstLocal) => {
                write(" (Intel RST local: AHCI somente leitura experimental)")
            }
            Some(carlio_os::pci::AhciModel::IntelNas) => {
                write(" (Intel AHCI NAS: somente leitura experimental)")
            }
            Some(carlio_os::pci::AhciModel::IntelQemu) => {
                write(" (QEMU: reconhecido; driver AHCI experimental)")
            }
            Some(carlio_os::pci::AhciModel::Unsupported) => {
                write(" (ID não suportado; sem acesso a disco)")
            }
            None => write(" (interface AHCI incompatível; sem acesso a disco)"),
        }
    } else if function.is_usb() {
        write(" USB");
    } else if function.is_network() {
        write(" rede");
        match function.network_model() {
            Some(carlio_os::pci::NetworkModel::RealtekRtl8111Nas) => {
                write(" (Realtek RTL8111 do servidor; driver pendente)")
            }
            Some(carlio_os::pci::NetworkModel::IntelI219Local) => {
                write(" (Intel I219-LM local; driver pendente)")
            }
            Some(carlio_os::pci::NetworkModel::Intel82574Qemu) => write(" (Intel 82574L/QEMU)"),
            Some(carlio_os::pci::NetworkModel::Unsupported) => {
                write(" (controlador não suportado)")
            }
            None => {}
        }
    } else if function.is_video() {
        write(" vídeo");
        match function.integrated_graphics_model() {
            Some(carlio_os::pci::IntegratedGraphicsModel::IntelHdGraphics520) => {
                write(" (Intel HD Graphics 520");
                report_intel_framebuffer_status();
            }
            Some(carlio_os::pci::IntegratedGraphicsModel::IntelIvyBridgeGt1) => {
                write(" (Intel Ivy Bridge GT1");
                report_intel_framebuffer_status();
            }
            Some(carlio_os::pci::IntegratedGraphicsModel::UnsupportedIntel) => {
                write(" (Intel iGPU sem suporte nativo)");
            }
            None => {}
        }
    }
    write("\r\n");
    if matches!(
        function.ahci_model(),
        Some(
            carlio_os::pci::AhciModel::IntelQemu
                | carlio_os::pci::AhciModel::IntelNas
                | carlio_os::pci::AhciModel::IntelRstLocal
        )
    ) {
        let hhdm = unsafe { core::ptr::addr_of!(HHDM_OFFSET).read_volatile() };
        let physical = unsafe { core::ptr::addr_of!(KERNEL_PHYSICAL_BASE).read_volatile() };
        let virtual_base = unsafe { core::ptr::addr_of!(KERNEL_VIRTUAL_BASE).read_volatile() };
        if hhdm != 0 && virtual_base != 0 {
            match unsafe { ahci::initialize(function, hhdm, physical, virtual_base) } {
                Ok(sectors) => {
                    write("AHCI: dispositivo identificado em leitura, setores=");
                    write_u64(sectors);
                    match ahci::block_device() {
                        Ok(device) => {
                            *AHCI_BLOCK_DEVICE.lock() = Some(device);
                            write(". Adaptador BlockDevice pronto para rootfs.\r\n");
                        }
                        Err(error) => {
                            write(". Adaptador indisponivel: ");
                            write(error.message());
                            write(".\r\n");
                        }
                    }
                }
                Err(error) => {
                    write("AHCI indisponível: ");
                    write(error.message());
                    write(".\r\n");
                }
            }
        } else {
            write("AHCI não iniciado: mapa DMA indisponível.\r\n");
        }
    }
    if function.network_model() == Some(carlio_os::pci::NetworkModel::Intel82574Qemu) {
        let hhdm = unsafe { core::ptr::addr_of!(HHDM_OFFSET).read_volatile() };
        let physical = unsafe { core::ptr::addr_of!(KERNEL_PHYSICAL_BASE).read_volatile() };
        let virtual_base = unsafe { core::ptr::addr_of!(KERNEL_VIRTUAL_BASE).read_volatile() };
        match unsafe { e1000::initialize(function, hhdm, physical, virtual_base) } {
            Ok(mac) => {
                write("NIC Intel 82574L: filas DMA TX/RX ativas, MAC=");
                for byte in mac {
                    write_hex_byte(byte);
                }
                write("; envio/recepcao polling disponiveis.\r\n");
                match e1000::request_dhcp_lease() {
                    Ok(Some(lease)) => {
                        write("DHCP IPv4: concessao recebida ");
                        for (index, octet) in lease.address.iter().enumerate() {
                            if index != 0 {
                                write(".");
                            }
                            write_u32(*octet as u32);
                        }
                        write("\r\n");
                    }
                    Ok(None) => {
                        write("DHCP IPv4: servidor sem resposta; interface sem endereco.\r\n")
                    }
                    Err(error) => {
                        write("DHCP IPv4 indisponivel: ");
                        write(error.message());
                        write(".\r\n");
                    }
                }
            }
            Err(error) => {
                write("NIC Intel 82574L indisponivel: ");
                write(error.message());
                write(".\r\n");
            }
        }
    }
}

#[cfg(target_os = "none")]
fn report_intel_framebuffer_status() {
    if FRAMEBUFFER_ACTIVE.load(Ordering::Acquire) {
        write("; framebuffer ativo, pixels escritos pela CPU)");
    } else {
        write("; framebuffer inativo, sem acesso gráfico)");
    }
}

#[cfg(target_os = "none")]
fn write_hex_byte(value: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    serial_write(HEX[(value >> 4) as usize]);
    serial_write(HEX[(value & 0x0f) as usize]);
}

#[cfg(target_os = "none")]
fn write_hex_nibble(value: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    serial_write(HEX[(value & 0x0f) as usize]);
}

#[cfg(target_os = "none")]
fn write_hex_word(value: u16) {
    write_hex_byte((value >> 8) as u8);
    write_hex_byte(value as u8);
}

#[cfg(target_os = "none")]
unsafe fn out(port: u16, value: u8) {
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
    }
}

#[cfg(target_os = "none")]
unsafe fn input(port: u16) -> u8 {
    let value: u8;
    unsafe {
        asm!("in al, dx", in("dx") port, out("al") value, options(nomem, nostack, preserves_flags));
    }
    value
}

#[cfg(target_os = "none")]
struct PanicSerial;

#[cfg(target_os = "none")]
impl FmtWrite for PanicSerial {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for byte in text.bytes() {
            if !serial_write(byte) {
                return Err(fmt::Error);
            }
        }
        Ok(())
    }
}

#[panic_handler]
#[cfg(target_os = "none")]
fn panic(info: &PanicInfo) -> ! {
    record_error(KernelError::KernelPanic);
    write("\r\n=== CAPTURA DE PANIC ===\r\n");
    write("Causa: ");
    let _ = PanicSerial.write_fmt(format_args!("{}\r\n", info.message()));
    if let Some(location) = info.location() {
        write("Local: ");
        write(location.file());
        write(":");
        write_u32(location.line());
        write(":");
        write_u32(location.column());
        write("\r\n");
    } else {
        write("Local: indisponível\r\n");
    }
    write("Registradores do ponto de panic indisponíveis; faults de CPU usam a captura IDT.\r\n");
    write("Captura somente serial e volátil; reiniciar perde este registro.\r\n");
    write("Modo de recuperação serial ativo; digite Enter para consultar erros.\r\n");
    loop {
        match serial_read() {
            Ok(byte) if byte == b'\r' || byte == b'\n' => {
                report_errors();
                write("Modo de recuperação ativo; núcleo sem operações de dispositivo.\r\n");
            }
            Ok(_) => {}
            Err(_) => core::hint::spin_loop(),
        }
    }
}

//! Driver polling experimental para o Intel 82574L apresentado pelo QEMU q35.
//! RTL8111/I219 físico e interrupções continuam fora deste driver.

use carlio_os::{
    errors::KernelError,
    net::{
        dns_a_answer, dns_a_query, encode_ipv4_udp, ipv4_next_hop, ArpPacket, EthernetFrame,
        Ipv4Packet, UdpDatagram,
    },
    pci::PciFunction,
};
use core::{
    arch::asm,
    ptr,
    sync::atomic::{compiler_fence, Ordering},
};

const RX_COUNT: usize = 16;
const TX_COUNT: usize = 16;
const BUFFER_SIZE: usize = 2048;
const POLL_LIMIT: usize = 2_000_000;
const CTRL: usize = 0x0000;
const RCTL: usize = 0x0100;
const TCTL: usize = 0x0400;
const TIPG: usize = 0x0410;
const RDBAL: usize = 0x2800;
const RDBAH: usize = 0x2804;
const RDLEN: usize = 0x2808;
const RDH: usize = 0x2810;
const RDT: usize = 0x2818;
const TDBAL: usize = 0x3800;
const TDBAH: usize = 0x3804;
const TDLEN: usize = 0x3808;
const TDH: usize = 0x3810;
const TDT: usize = 0x3818;
const RAL: usize = 0x5400;
const RAH: usize = 0x5404;
const IMS: usize = 0x00d0;
const IMC: usize = 0x00d8;
const CTRL_RST: u32 = 1 << 26;
const RX_DD: u8 = 1;
const RX_EOP: u8 = 2;
const TX_DD: u8 = 1;
const TX_CMD_EOP_IFCS_RS: u8 = 0x0b;
const DHCP_POLL_LIMIT: usize = 20_000_000;
const ARP_POLL_LIMIT: usize = 2_000_000;
const DNS_POLL_LIMIT: usize = 200_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DhcpLease {
    pub address: [u8; 4],
    pub subnet_mask: Option<[u8; 4]>,
    pub router: Option<[u8; 4]>,
    pub dns_server: Option<[u8; 4]>,
    pub lease_seconds: Option<u32>,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RxDescriptor {
    address: u64,
    length: u16,
    checksum: u16,
    status: u8,
    errors: u8,
    special: u16,
}
impl RxDescriptor {
    const EMPTY: Self = Self {
        address: 0,
        length: 0,
        checksum: 0,
        status: 0,
        errors: 0,
        special: 0,
    };
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TxDescriptor {
    address: u64,
    length: u16,
    checksum_offset: u8,
    command: u8,
    status: u8,
    checksum_start: u8,
    special: u16,
}
impl TxDescriptor {
    const EMPTY: Self = Self {
        address: 0,
        length: 0,
        checksum_offset: 0,
        command: 0,
        status: TX_DD,
        checksum_start: 0,
        special: 0,
    };
}

#[repr(C, align(4096))]
struct Dma {
    rx: [RxDescriptor; RX_COUNT],
    tx: [TxDescriptor; TX_COUNT],
    rx_buffers: [[u8; BUFFER_SIZE]; RX_COUNT],
    tx_buffers: [[u8; BUFFER_SIZE]; TX_COUNT],
}

static mut DMA: Dma = Dma {
    rx: [RxDescriptor::EMPTY; RX_COUNT],
    tx: [TxDescriptor::EMPTY; TX_COUNT],
    rx_buffers: [[0; BUFFER_SIZE]; RX_COUNT],
    tx_buffers: [[0; BUFFER_SIZE]; TX_COUNT],
};
static mut NIC: Option<Controller> = None;

#[derive(Clone, Copy)]
struct Controller {
    registers: *mut u8,
    rx_index: usize,
    tx_index: usize,
    mac: [u8; 6],
}

pub unsafe fn initialize(
    function: PciFunction,
    hhdm: u64,
    kernel_physical: u64,
    kernel_virtual: u64,
) -> Result<[u8; 6], KernelError> {
    if function.vendor_id != 0x8086 || function.device_id != 0x10d3 {
        return Err(KernelError::UnsupportedDevice);
    }
    let command = pci_read(function, 0x04) as u16;
    pci_write(function, 0x04, (command | 0x0006) as u32);
    let registers =
        unsafe { super::ahci::map_pci_bar(function, 0x10, hhdm, kernel_physical, kernel_virtual)? };
    unsafe {
        write32(registers, IMC, u32::MAX);
        let _ = read32(registers, IMS);
    }
    let ctrl = unsafe { read32(registers, CTRL) };
    unsafe {
        write32(registers, CTRL, ctrl | CTRL_RST);
    }
    let mut reset = false;
    for _ in 0..POLL_LIMIT {
        if unsafe { read32(registers, CTRL) } & CTRL_RST == 0 {
            reset = true;
            break;
        }
    }
    if !reset {
        return Err(KernelError::IoTimeout);
    }
    unsafe {
        write32(registers, IMC, u32::MAX);
        let _ = read32(registers, IMS);
    }

    let ral = unsafe { read32(registers, RAL) }.to_le_bytes();
    let rah = unsafe { read32(registers, RAH) }.to_le_bytes();
    let mac = [ral[0], ral[1], ral[2], ral[3], rah[0], rah[1]];
    if mac == [0; 6] || mac == [0xff; 6] {
        return Err(KernelError::DeviceInitFailed);
    }

    for i in 0..RX_COUNT {
        let buffer = core::ptr::addr_of_mut!(DMA.rx_buffers[i]) as u64;
        let phys = virtual_to_physical(buffer, kernel_physical, kernel_virtual)?;
        unsafe {
            ptr::addr_of_mut!(DMA.rx[i]).write(RxDescriptor {
                address: phys,
                ..RxDescriptor::EMPTY
            });
        }
    }
    for i in 0..TX_COUNT {
        let buffer = core::ptr::addr_of_mut!(DMA.tx_buffers[i]) as u64;
        let phys = virtual_to_physical(buffer, kernel_physical, kernel_virtual)?;
        unsafe {
            ptr::addr_of_mut!(DMA.tx[i]).write(TxDescriptor {
                address: phys,
                ..TxDescriptor::EMPTY
            });
        }
    }
    let rx_phys = virtual_to_physical(
        core::ptr::addr_of!(DMA.rx) as u64,
        kernel_physical,
        kernel_virtual,
    )?;
    let tx_phys = virtual_to_physical(
        core::ptr::addr_of!(DMA.tx) as u64,
        kernel_physical,
        kernel_virtual,
    )?;
    unsafe {
        write32(registers, RDBAL, rx_phys as u32);
        write32(registers, RDBAH, (rx_phys >> 32) as u32);
        write32(
            registers,
            RDLEN,
            (RX_COUNT * size_of::<RxDescriptor>()) as u32,
        );
        write32(registers, RDH, 0);
        write32(registers, RDT, (RX_COUNT - 1) as u32);
        write32(registers, TDBAL, tx_phys as u32);
        write32(registers, TDBAH, (tx_phys >> 32) as u32);
        write32(
            registers,
            TDLEN,
            (TX_COUNT * size_of::<TxDescriptor>()) as u32,
        );
        write32(registers, TDH, 0);
        write32(registers, TDT, 0);
        write32(registers, TIPG, 0x0060_200a);
        write32(
            registers,
            TCTL,
            (1 << 1) | (1 << 3) | (0x0f << 4) | (0x40 << 12),
        );
        write32(registers, RCTL, (1 << 1) | (1 << 15) | (1 << 26));
        compiler_fence(Ordering::SeqCst);
    }
    unsafe {
        NIC = Some(Controller {
            registers,
            rx_index: 0,
            tx_index: 0,
            mac,
        });
    }
    Ok(mac)
}

/// Enfileira e aguarda um frame Ethernet, sem fragmentação de software.
pub fn transmit(frame: &[u8]) -> Result<(), KernelError> {
    if frame.len() < 14 || frame.len() > BUFFER_SIZE {
        return Err(KernelError::ControllerError);
    }
    let mut nic = unsafe { NIC }.ok_or(KernelError::DeviceNotFound)?;
    let index = nic.tx_index;
    let descriptor = unsafe { ptr::addr_of_mut!(DMA.tx[index]) };
    if unsafe { ptr::read_volatile(ptr::addr_of!((*descriptor).status)) } & TX_DD == 0 {
        return Err(KernelError::IoTimeout);
    }
    let buffer = unsafe { ptr::addr_of_mut!(DMA.tx_buffers[index]).cast::<u8>() };
    unsafe {
        ptr::copy_nonoverlapping(frame.as_ptr(), buffer, frame.len());
    }
    unsafe {
        ptr::write_volatile(ptr::addr_of_mut!((*descriptor).length), frame.len() as u16);
        ptr::write_volatile(ptr::addr_of_mut!((*descriptor).command), TX_CMD_EOP_IFCS_RS);
        ptr::write_volatile(ptr::addr_of_mut!((*descriptor).status), 0);
        compiler_fence(Ordering::Release);
        write32(nic.registers, TDT, ((index + 1) % TX_COUNT) as u32);
    }
    let mut complete = false;
    for _ in 0..POLL_LIMIT {
        if unsafe { ptr::read_volatile(ptr::addr_of!((*descriptor).status)) } & TX_DD != 0 {
            complete = true;
            break;
        }
    }
    if !complete {
        return Err(KernelError::IoTimeout);
    }
    nic.tx_index = (index + 1) % TX_COUNT;
    unsafe {
        NIC = Some(nic);
    }
    Ok(())
}

/// Recebe um frame completo; retorna None quando a fila está vazia.
pub fn receive(output: &mut [u8]) -> Result<Option<usize>, KernelError> {
    let mut nic = unsafe { NIC }.ok_or(KernelError::DeviceNotFound)?;
    let index = nic.rx_index;
    let descriptor = unsafe { ptr::addr_of_mut!(DMA.rx[index]) };
    let status = unsafe { ptr::read_volatile(ptr::addr_of!((*descriptor).status)) };
    if status & RX_DD == 0 {
        return Ok(None);
    }
    let length = unsafe { ptr::read_volatile(ptr::addr_of!((*descriptor).length)) } as usize;
    let errors = unsafe { ptr::read_volatile(ptr::addr_of!((*descriptor).errors)) };
    if status & RX_EOP == 0
        || errors != 0
        || length < 14
        || length > BUFFER_SIZE
        || output.len() < length
    {
        return Err(KernelError::ControllerError);
    }
    let source = unsafe { ptr::addr_of!(DMA.rx_buffers[index]).cast::<u8>() };
    unsafe {
        ptr::copy_nonoverlapping(source, output.as_mut_ptr(), length);
    }
    unsafe {
        ptr::write_volatile(ptr::addr_of_mut!((*descriptor).status), 0);
        ptr::write_volatile(ptr::addr_of_mut!((*descriptor).length), 0);
        compiler_fence(Ordering::Release);
        write32(nic.registers, RDT, index as u32);
    }
    nic.rx_index = (index + 1) % RX_COUNT;
    unsafe {
        NIC = Some(nic);
    }
    Ok(Some(length))
}

/// Executa DHCPv4 DORA em polling e retorna somente após DHCPACK.
pub fn request_dhcp_lease() -> Result<Option<DhcpLease>, KernelError> {
    let nic = unsafe { NIC }.ok_or(KernelError::DeviceNotFound)?;
    let mut frame = [0u8; BUFFER_SIZE];
    let discover = build_dhcp_frame(nic.mac, 1, None, None, &mut frame)?;
    transmit(&frame[..discover])?;
    let offered = wait_dhcp_message(&mut frame, nic.mac, 2)?;
    let Some((address, server, _)) = offered else {
        return Ok(None);
    };
    let request = build_dhcp_frame(nic.mac, 3, Some(address), Some(server), &mut frame)?;
    transmit(&frame[..request])?;
    let acknowledged = wait_dhcp_message(&mut frame, nic.mac, 5)?;
    let Some((leased_address, _, options)) = acknowledged else {
        return Ok(None);
    };
    Ok(Some(DhcpLease {
        address: leased_address,
        subnet_mask: options.subnet_mask,
        router: options.router,
        dns_server: options.dns_server,
        lease_seconds: options.lease_seconds,
    }))
}

/// Prova a conectividade local com o roteador recebido por DHCP.
pub fn resolve_gateway(lease: DhcpLease) -> Result<Option<[u8; 6]>, KernelError> {
    let Some(gateway) = lease.router.filter(|gateway| *gateway != [0; 4]) else {
        return Ok(None);
    };
    resolve_neighbor(lease.address, gateway)
}

fn resolve_neighbor(
    local_address: [u8; 4],
    next_hop: [u8; 4],
) -> Result<Option<[u8; 6]>, KernelError> {
    let nic = unsafe { NIC }.ok_or(KernelError::DeviceNotFound)?;
    let mut frame = [0; BUFFER_SIZE];

    frame[..6].fill(0xff);
    frame[6..12].copy_from_slice(&nic.mac);
    frame[12..14].copy_from_slice(&0x0806_u16.to_be_bytes());
    frame[14..22].copy_from_slice(&[0, 1, 8, 0, 6, 4, 0, 1]);
    frame[22..28].copy_from_slice(&nic.mac);
    frame[28..32].copy_from_slice(&local_address);
    frame[38..42].copy_from_slice(&next_hop);

    transmit(&frame[..60])?;
    for _ in 0..ARP_POLL_LIMIT {
        let Some(length) = receive(&mut frame)? else {
            core::hint::spin_loop();
            continue;
        };
        let Ok(ethernet) = EthernetFrame::parse(&frame[..length]) else {
            continue;
        };
        if ethernet.ethertype != 0x0806 || ethernet.destination != nic.mac {
            continue;
        }
        let Ok(reply) = ArpPacket::parse(ethernet.payload) else {
            continue;
        };
        if reply.operation == 2
            && reply.sender_ipv4 == next_hop
            && reply.sender_mac == ethernet.source
            && reply.sender_mac != [0; 6]
            && reply.sender_mac[0] & 1 == 0
            && reply.target_ipv4 == local_address
            && reply.target_mac == nic.mac
        {
            return Ok(Some(reply.sender_mac));
        }
    }
    Ok(None)
}

/// Resolve uma consulta A direta durante a inicialização, com a interface em polling.
pub fn resolve_dns_a_boot(lease: DhcpLease, name: &str) -> Result<Option<[u8; 4]>, KernelError> {
    let mut query = [0; 271];
    // O diagnóstico roda uma vez, no BSP, antes de expor a interface a processos.
    let stamp = unsafe { core::arch::x86_64::_rdtsc() };
    let source_port = 49152 | ((stamp >> 16) as u16 & 0x3fff);
    let query_len =
        dns_a_query(name, stamp as u16, &mut query).map_err(|_| KernelError::ControllerError)?;
    let query = &query[..query_len];
    let Some(server) = lease.dns_server else {
        return Ok(None);
    };
    let Some(next_hop) = ipv4_next_hop(lease.address, lease.subnet_mask, lease.router, server)
    else {
        return Ok(None);
    };
    let Some(next_mac) = resolve_neighbor(lease.address, next_hop)? else {
        return Ok(None);
    };
    let nic = unsafe { NIC }.ok_or(KernelError::DeviceNotFound)?;
    let mut frame = [0; BUFFER_SIZE];
    frame[..6].copy_from_slice(&next_mac);
    frame[6..12].copy_from_slice(&nic.mac);
    frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
    let length = encode_ipv4_udp(
        lease.address,
        server,
        source_port,
        53,
        query,
        &mut frame[14..],
    )
    .map_err(|_| KernelError::ControllerError)?;
    transmit(&frame[..(14 + length).max(60)])?;
    for _ in 0..DNS_POLL_LIMIT {
        let Some(length) = receive(&mut frame)? else {
            core::hint::spin_loop();
            continue;
        };
        let Ok(ethernet) = EthernetFrame::parse(&frame[..length]) else {
            continue;
        };
        if ethernet.ethertype != 0x0800 || ethernet.destination != nic.mac {
            continue;
        }
        let Ok(ip) = Ipv4Packet::parse(ethernet.payload) else {
            continue;
        };
        if ip.protocol != 17 || ip.source != server || ip.destination != lease.address {
            continue;
        }
        let Ok(udp) = UdpDatagram::parse(ip.payload, ip.source, ip.destination) else {
            continue;
        };
        if udp.source_port == 53 && udp.destination_port == source_port {
            if let Some(address) = dns_a_answer(query, udp.payload) {
                return Ok(Some(address));
            }
        }
    }
    Ok(None)
}

fn build_dhcp_frame(
    mac: [u8; 6],
    message_type: u8,
    requested_ip: Option<[u8; 4]>,
    server: Option<[u8; 4]>,
    frame: &mut [u8; BUFFER_SIZE],
) -> Result<usize, KernelError> {
    use carlio_os::net::{checksum, ETHERNET_HEADER_LEN};
    let bootp = 42;
    frame.fill(0);
    frame[..6].fill(0xff);
    frame[6..12].copy_from_slice(&mac);
    frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
    let ip = 14;
    frame[ip] = 0x45;
    frame[ip + 8] = 64;
    frame[ip + 9] = 17;
    frame[ip + 12..ip + 16].copy_from_slice(&[0; 4]);
    frame[ip + 16..ip + 20].copy_from_slice(&[255; 4]);
    let udp = ip + 20;
    frame[udp..udp + 2].copy_from_slice(&68u16.to_be_bytes());
    frame[udp + 2..udp + 4].copy_from_slice(&67u16.to_be_bytes());
    let payload = bootp;
    frame[payload] = 1;
    frame[payload + 1] = 1;
    frame[payload + 2] = 6;
    frame[payload + 4..payload + 8].copy_from_slice(&0x4341_524c_u32.to_be_bytes());
    frame[payload + 10..payload + 12].copy_from_slice(&0x8000u16.to_be_bytes());
    frame[payload + 28..payload + 34].copy_from_slice(&mac);
    frame[payload + 236..payload + 240].copy_from_slice(&[99, 130, 83, 99]);
    let mut cursor = payload + 240;
    frame[cursor..cursor + 3].copy_from_slice(&[53, 1, message_type]);
    cursor += 3;
    if let Some(address) = requested_ip {
        frame[cursor..cursor + 6]
            .copy_from_slice(&[50, 4, address[0], address[1], address[2], address[3]]);
        cursor += 6;
    }
    if let Some(server) = server {
        frame[cursor..cursor + 6]
            .copy_from_slice(&[54, 4, server[0], server[1], server[2], server[3]]);
        cursor += 6;
    }
    frame[cursor..cursor + 7].copy_from_slice(&[55, 4, 1, 3, 6, 51, 255]);
    cursor += 7;
    frame[cursor] = 255;
    cursor += 1;
    let udp_length = cursor - udp;
    frame[udp + 4..udp + 6].copy_from_slice(&(udp_length as u16).to_be_bytes());
    let ip_length = 20 + udp_length;
    frame[ip + 2..ip + 4].copy_from_slice(&(ip_length as u16).to_be_bytes());
    let ip_checksum = checksum(&frame[ip..ip + 20]);
    frame[ip + 10..ip + 12].copy_from_slice(&ip_checksum.to_be_bytes());
    let frame_length = ETHERNET_HEADER_LEN + ip_length;
    if frame_length > frame.len() {
        return Err(KernelError::ControllerError);
    }
    Ok(frame_length)
}

fn wait_dhcp_message(
    frame: &mut [u8; BUFFER_SIZE],
    mac: [u8; 6],
    wanted_type: u8,
) -> Result<Option<([u8; 4], [u8; 4], carlio_os::net::DhcpOptions)>, KernelError> {
    for _ in 0..DHCP_POLL_LIMIT {
        match receive(frame)? {
            Some(length) => {
                let Ok(ethernet) = carlio_os::net::EthernetFrame::parse(&frame[..length]) else {
                    continue;
                };
                if ethernet.ethertype != 0x0800 {
                    continue;
                }
                let Ok(ip) = carlio_os::net::Ipv4Packet::parse(ethernet.payload) else {
                    continue;
                };
                if ip.protocol != 17 {
                    continue;
                }
                let Ok(udp) =
                    carlio_os::net::UdpDatagram::parse(ip.payload, ip.source, ip.destination)
                else {
                    continue;
                };
                if udp.source_port != 67 || udp.destination_port != 68 || udp.payload.len() < 240 {
                    continue;
                }
                if udp.payload[0] != 2
                    || udp.payload[2] != 6
                    || udp.payload[4..8] != 0x4341_524c_u32.to_be_bytes()
                    || udp.payload[28..34] != mac
                {
                    continue;
                }
                let Ok(options) = carlio_os::net::DhcpOptions::parse(udp.payload) else {
                    continue;
                };
                if options.message_type != Some(wanted_type) {
                    continue;
                }
                let address = udp.payload[16..20].try_into().unwrap_or([0; 4]);
                let server = options.server_id.unwrap_or([0; 4]);
                return Ok(Some((address, server, options)));
            }
            None => core::hint::spin_loop(),
        }
    }
    Ok(None)
}

fn virtual_to_physical(
    address: u64,
    physical_base: u64,
    virtual_base: u64,
) -> Result<u64, KernelError> {
    physical_base
        .checked_add(
            address
                .checked_sub(virtual_base)
                .ok_or(KernelError::DeviceInitFailed)?,
        )
        .ok_or(KernelError::DeviceInitFailed)
}

unsafe fn read32(base: *mut u8, offset: usize) -> u32 {
    unsafe { ptr::read_volatile(base.add(offset).cast::<u32>()) }
}
unsafe fn write32(base: *mut u8, offset: usize, value: u32) {
    unsafe { ptr::write_volatile(base.add(offset).cast::<u32>(), value) };
}

fn pci_read(function: PciFunction, offset: u8) -> u32 {
    let address = 0x8000_0000u32
        | ((function.bus as u32) << 16)
        | ((function.device as u32) << 11)
        | ((function.function as u32) << 8)
        | ((offset as u32) & 0xfc);
    unsafe {
        out32(0xcf8, address);
        in32(0xcfc)
    }
}
fn pci_write(function: PciFunction, offset: u8, value: u32) {
    let address = 0x8000_0000u32
        | ((function.bus as u32) << 16)
        | ((function.device as u32) << 11)
        | ((function.function as u32) << 8)
        | ((offset as u32) & 0xfc);
    unsafe {
        out32(0xcf8, address);
        out32(0xcfc, value);
    }
}
unsafe fn out32(port: u16, value: u32) {
    unsafe {
        asm!("out dx, eax", in("dx") port, in("eax") value, options(nomem, nostack, preserves_flags));
    }
}
unsafe fn in32(port: u16) -> u32 {
    let value;
    unsafe {
        asm!("in eax, dx", in("dx") port, out("eax") value, options(nomem, nostack, preserves_flags));
    }
    value
}

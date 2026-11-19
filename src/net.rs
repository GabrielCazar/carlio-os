//! Decodificadores bounded para Ethernet, ARP, IPv4, UDP e opções DHCP.
//!
//! Este módulo não fornece driver de placa, filas DMA, roteamento ou sockets;
//! ele valida quadros recebidos antes de uma futura integração com NIC.

pub const ETHERNET_HEADER_LEN: usize = 14;
pub const IPV4_MIN_HEADER_LEN: usize = 20;
pub const UDP_HEADER_LEN: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkError {
    Truncated,
    Unsupported,
    InvalidLength,
    InvalidChecksum,
    Fragmented,
    InvalidAddress,
    InvalidOption,
    AddressInUse,
    PermissionDenied,
    BadDescriptor,
    UnsupportedSocket,
    InvalidPort,
    InvalidState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketProtocol {
    Tcp,
    Udp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketState {
    Created,
    Bound,
    Listening,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SocketInfo {
    pub process_id: u64,
    pub descriptor: u32,
    pub protocol: SocketProtocol,
    pub state: SocketState,
    pub local_address: [u8; 4],
    pub local_port: u16,
    pub backlog: u16,
}

#[derive(Clone, Copy)]
struct SocketEntry {
    info: SocketInfo,
    occupied: bool,
}
impl SocketEntry {
    const EMPTY: Self = Self {
        info: SocketInfo {
            process_id: 0,
            descriptor: 0,
            protocol: SocketProtocol::Udp,
            state: SocketState::Created,
            local_address: [0; 4],
            local_port: 0,
            backlog: 0,
        },
        occupied: false,
    };
}

/// Tabela bounded de endpoints IPv4 e portas de transporte, por processo.
/// Ainda não move pacotes; RX/TX depende do driver físico.
pub struct SocketTable<const N: usize> {
    entries: [SocketEntry; N],
}
impl<const N: usize> SocketTable<N> {
    pub const fn new() -> Self {
        Self {
            entries: [SocketEntry::EMPTY; N],
        }
    }

    pub fn create(
        &mut self,
        process_id: u64,
        uid: u32,
        domain: u64,
        kind: u64,
        protocol: u64,
    ) -> Result<u32, NetworkError> {
        if domain != 2 || (kind != 1 && kind != 2) {
            return Err(NetworkError::UnsupportedSocket);
        }
        let transport = match (kind, protocol) {
            (1, 0 | 6) => SocketProtocol::Tcp,
            (2, 0 | 17) => SocketProtocol::Udp,
            _ => return Err(NetworkError::UnsupportedSocket),
        };
        let _ = uid;
        let slot = self
            .entries
            .iter()
            .position(|entry| !entry.occupied)
            .ok_or(NetworkError::InvalidState)?;
        let descriptor = (3..3 + N as u32)
            .find(|fd| {
                !self.entries.iter().any(|entry| {
                    entry.occupied
                        && entry.info.process_id == process_id
                        && entry.info.descriptor == *fd
                })
            })
            .ok_or(NetworkError::InvalidState)?;
        self.entries[slot] = SocketEntry {
            occupied: true,
            info: SocketInfo {
                process_id,
                descriptor,
                protocol: transport,
                state: SocketState::Created,
                local_address: [0; 4],
                local_port: 0,
                backlog: 0,
            },
        };
        Ok(descriptor)
    }

    pub fn bind(
        &mut self,
        process_id: u64,
        uid: u32,
        descriptor: u32,
        address: [u8; 4],
        port: u16,
    ) -> Result<(), NetworkError> {
        if port == 0 {
            return Err(NetworkError::InvalidPort);
        }
        if port < 1024 && uid != 0 {
            return Err(NetworkError::PermissionDenied);
        }
        let index = self
            .find(process_id, descriptor)
            .ok_or(NetworkError::BadDescriptor)?;
        if self.entries[index].info.state != SocketState::Created {
            return Err(NetworkError::InvalidState);
        }
        let candidate = self.entries[index].info;
        if self.entries.iter().any(|entry| {
            entry.occupied
                && entry.info.protocol == candidate.protocol
                && entry.info.local_port == port
                && (address == [0; 4]
                    || entry.info.local_address == [0; 4]
                    || entry.info.local_address == address)
        }) {
            return Err(NetworkError::AddressInUse);
        }
        self.entries[index].info.local_address = address;
        self.entries[index].info.local_port = port;
        self.entries[index].info.state = SocketState::Bound;
        Ok(())
    }

    pub fn listen(
        &mut self,
        process_id: u64,
        descriptor: u32,
        backlog: u16,
    ) -> Result<(), NetworkError> {
        let index = self
            .find(process_id, descriptor)
            .ok_or(NetworkError::BadDescriptor)?;
        let info = self.entries[index].info;
        if info.protocol != SocketProtocol::Tcp || info.state != SocketState::Bound || backlog == 0
        {
            return Err(NetworkError::InvalidState);
        }
        self.entries[index].info.state = SocketState::Listening;
        self.entries[index].info.backlog = backlog.min(128);
        Ok(())
    }

    pub fn close(&mut self, process_id: u64, descriptor: u32) -> Result<(), NetworkError> {
        let index = self
            .find(process_id, descriptor)
            .ok_or(NetworkError::BadDescriptor)?;
        self.entries[index] = SocketEntry::EMPTY;
        Ok(())
    }

    pub fn close_process(&mut self, process_id: u64) -> usize {
        if process_id == 0 {
            return 0;
        }
        let mut closed = 0usize;
        for entry in &mut self.entries {
            if entry.occupied && entry.info.process_id == process_id {
                *entry = SocketEntry::EMPTY;
                closed += 1;
            }
        }
        closed
    }

    pub fn info(&self, process_id: u64, descriptor: u32) -> Option<SocketInfo> {
        self.find(process_id, descriptor)
            .map(|index| self.entries[index].info)
    }

    fn find(&self, process_id: u64, descriptor: u32) -> Option<usize> {
        self.entries.iter().position(|entry| {
            entry.occupied
                && entry.info.process_id == process_id
                && entry.info.descriptor == descriptor
        })
    }
}

impl<const N: usize> Default for SocketTable<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EthernetFrame<'a> {
    pub destination: [u8; 6],
    pub source: [u8; 6],
    pub ethertype: u16,
    pub payload: &'a [u8],
}

impl<'a> EthernetFrame<'a> {
    pub fn parse(frame: &'a [u8]) -> Result<Self, NetworkError> {
        if frame.len() < ETHERNET_HEADER_LEN {
            return Err(NetworkError::Truncated);
        }
        let mut destination = [0; 6];
        destination.copy_from_slice(&frame[..6]);
        let mut source = [0; 6];
        source.copy_from_slice(&frame[6..12]);
        let ethertype = u16::from_be_bytes([frame[12], frame[13]]);
        if ethertype == 0x8100 || ethertype == 0x88a8 {
            return Err(NetworkError::Unsupported);
        }
        Ok(Self {
            destination,
            source,
            ethertype,
            payload: &frame[14..],
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArpPacket {
    pub operation: u16,
    pub sender_mac: [u8; 6],
    pub sender_ipv4: [u8; 4],
    pub target_mac: [u8; 6],
    pub target_ipv4: [u8; 4],
}

impl ArpPacket {
    pub fn parse(bytes: &[u8]) -> Result<Self, NetworkError> {
        if bytes.len() < 28 {
            return Err(NetworkError::Truncated);
        }
        if u16be(bytes, 0) != 1 || u16be(bytes, 2) != 0x0800 || bytes[4] != 6 || bytes[5] != 4 {
            return Err(NetworkError::Unsupported);
        }
        let operation = u16be(bytes, 6);
        if operation != 1 && operation != 2 {
            return Err(NetworkError::Unsupported);
        }
        Ok(Self {
            operation,
            sender_mac: bytes[8..14]
                .try_into()
                .map_err(|_| NetworkError::Truncated)?,
            sender_ipv4: bytes[14..18]
                .try_into()
                .map_err(|_| NetworkError::Truncated)?,
            target_mac: bytes[18..24]
                .try_into()
                .map_err(|_| NetworkError::Truncated)?,
            target_ipv4: bytes[24..28]
                .try_into()
                .map_err(|_| NetworkError::Truncated)?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ipv4Packet<'a> {
    pub source: [u8; 4],
    pub destination: [u8; 4],
    pub protocol: u8,
    pub ttl: u8,
    pub payload: &'a [u8],
}

impl<'a> Ipv4Packet<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, NetworkError> {
        if bytes.len() < IPV4_MIN_HEADER_LEN {
            return Err(NetworkError::Truncated);
        }
        if bytes[0] >> 4 != 4 {
            return Err(NetworkError::Unsupported);
        }
        let header_len = ((bytes[0] & 0x0f) as usize) * 4;
        let total_len = u16be(bytes, 2) as usize;
        if header_len < IPV4_MIN_HEADER_LEN || header_len > bytes.len() || total_len < header_len {
            return Err(NetworkError::InvalidLength);
        }
        if total_len > bytes.len() {
            return Err(NetworkError::Truncated);
        }
        if checksum(&bytes[..header_len]) != 0 {
            return Err(NetworkError::InvalidChecksum);
        }
        let fragment = u16be(bytes, 6);
        if fragment & 0x3fff != 0 {
            return Err(NetworkError::Fragmented);
        }
        Ok(Self {
            source: bytes[12..16]
                .try_into()
                .map_err(|_| NetworkError::Truncated)?,
            destination: bytes[16..20]
                .try_into()
                .map_err(|_| NetworkError::Truncated)?,
            protocol: bytes[9],
            ttl: bytes[8],
            payload: &bytes[header_len..total_len],
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UdpDatagram<'a> {
    pub source_port: u16,
    pub destination_port: u16,
    pub payload: &'a [u8],
}

impl<'a> UdpDatagram<'a> {
    /// Verifica o checksum UDP quando presente. IPv4 permite checksum zero.
    pub fn parse(
        bytes: &'a [u8],
        source: [u8; 4],
        destination: [u8; 4],
    ) -> Result<Self, NetworkError> {
        if bytes.len() < UDP_HEADER_LEN {
            return Err(NetworkError::Truncated);
        }
        let length = u16be(bytes, 4) as usize;
        if length < UDP_HEADER_LEN {
            return Err(NetworkError::InvalidLength);
        }
        if length > bytes.len() {
            return Err(NetworkError::Truncated);
        }
        let checksum_field = u16be(bytes, 6);
        if checksum_field != 0 && udp_checksum(source, destination, &bytes[..length]) != 0 {
            return Err(NetworkError::InvalidChecksum);
        }
        Ok(Self {
            source_port: u16be(bytes, 0),
            destination_port: u16be(bytes, 2),
            payload: &bytes[8..length],
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DhcpOptions {
    pub message_type: Option<u8>,
    pub requested_ip: Option<[u8; 4]>,
    pub server_id: Option<[u8; 4]>,
    pub subnet_mask: Option<[u8; 4]>,
    pub router: Option<[u8; 4]>,
    pub dns_server: Option<[u8; 4]>,
    pub lease_seconds: Option<u32>,
}

impl DhcpOptions {
    /// Lê o campo options BOOTP/DHCP, incluindo o cookie mágico RFC 2131.
    pub fn parse(bootp: &[u8]) -> Result<Self, NetworkError> {
        if bootp.len() < 240 {
            return Err(NetworkError::Truncated);
        }
        if bootp[236..240] != [99, 130, 83, 99] {
            return Err(NetworkError::InvalidOption);
        }
        let mut result = Self::default();
        let mut i = 240;
        let mut ended = false;
        while i < bootp.len() {
            let code = bootp[i];
            i += 1;
            if code == 0 {
                continue;
            }
            if code == 255 {
                ended = true;
                break;
            }
            let len = *bootp.get(i).ok_or(NetworkError::Truncated)? as usize;
            i += 1;
            let end = i
                .checked_add(len)
                .filter(|end| *end <= bootp.len())
                .ok_or(NetworkError::Truncated)?;
            let value = &bootp[i..end];
            i = end;
            match (code, value.len()) {
                (53, 1) => set_once(&mut result.message_type, value[0])?,
                (50, 4) => set_once(
                    &mut result.requested_ip,
                    value.try_into().map_err(|_| NetworkError::InvalidOption)?,
                )?,
                (54, 4) => set_once(
                    &mut result.server_id,
                    value.try_into().map_err(|_| NetworkError::InvalidOption)?,
                )?,
                (1, 4) => set_once(
                    &mut result.subnet_mask,
                    value.try_into().map_err(|_| NetworkError::InvalidOption)?,
                )?,
                (3, n) if n >= 4 && n % 4 == 0 => set_once(
                    &mut result.router,
                    value[..4]
                        .try_into()
                        .map_err(|_| NetworkError::InvalidOption)?,
                )?,
                (6, n) if n >= 4 && n % 4 == 0 => set_once(
                    &mut result.dns_server,
                    value[..4]
                        .try_into()
                        .map_err(|_| NetworkError::InvalidOption)?,
                )?,
                (51, 4) => set_once(
                    &mut result.lease_seconds,
                    u32::from_be_bytes(value.try_into().map_err(|_| NetworkError::InvalidOption)?),
                )?,
                (53 | 50 | 54 | 1 | 3 | 6 | 51, _) => return Err(NetworkError::InvalidOption),
                _ => {}
            }
        }
        if !ended {
            return Err(NetworkError::InvalidOption);
        }
        Ok(result)
    }
}

fn set_once<T: Copy>(slot: &mut Option<T>, value: T) -> Result<(), NetworkError> {
    if slot.is_some() {
        return Err(NetworkError::InvalidOption);
    }
    *slot = Some(value);
    Ok(())
}

/// Internet checksum RFC 1071; valid data including its checksum returns zero.
pub fn checksum(bytes: &[u8]) -> u16 {
    let mut sum = 0u32;
    let (chunks, remainder) = bytes.as_chunks::<2>();
    for pair in chunks {
        sum += u16::from_be_bytes([pair[0], pair[1]]) as u32;
    }
    if let Some(last) = remainder.first() {
        sum += (*last as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

fn udp_checksum(source: [u8; 4], destination: [u8; 4], udp: &[u8]) -> u16 {
    let mut sum = 0u32;
    for address in [source, destination] {
        sum += u16::from_be_bytes([address[0], address[1]]) as u32;
        sum += u16::from_be_bytes([address[2], address[3]]) as u32;
    }
    sum += 17;
    sum += udp.len() as u32;
    let (chunks, remainder) = udp.as_chunks::<2>();
    for pair in chunks {
        sum += u16::from_be_bytes([pair[0], pair[1]]) as u32;
    }
    if let Some(last) = remainder.first() {
        sum += (*last as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

fn u16be(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([bytes[offset], bytes[offset + 1]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ethernet_arp_and_ipv4_udp() {
        let mut arp = [0u8; 28];
        arp[0..2].copy_from_slice(&1u16.to_be_bytes());
        arp[2..4].copy_from_slice(&0x0800u16.to_be_bytes());
        arp[4] = 6;
        arp[5] = 4;
        arp[6..8].copy_from_slice(&1u16.to_be_bytes());
        arp[8..14].copy_from_slice(&[1, 2, 3, 4, 5, 6]);
        arp[14..18].copy_from_slice(&[192, 168, 1, 2]);
        let parsed = ArpPacket::parse(&arp).unwrap();
        assert_eq!(parsed.sender_ipv4, [192, 168, 1, 2]);

        let mut ip = [0u8; 28];
        ip[0] = 0x45;
        ip[2..4].copy_from_slice(&28u16.to_be_bytes());
        ip[8] = 64;
        ip[9] = 17;
        ip[12..16].copy_from_slice(&[192, 168, 1, 2]);
        ip[16..20].copy_from_slice(&[192, 168, 1, 1]);
        let sum = checksum(&ip[..20]);
        ip[10..12].copy_from_slice(&sum.to_be_bytes());
        ip[20..22].copy_from_slice(&68u16.to_be_bytes());
        ip[22..24].copy_from_slice(&67u16.to_be_bytes());
        ip[24..26].copy_from_slice(&8u16.to_be_bytes());
        let parsed_ip = Ipv4Packet::parse(&ip).unwrap();
        let udp =
            UdpDatagram::parse(parsed_ip.payload, parsed_ip.source, parsed_ip.destination).unwrap();
        assert_eq!((udp.source_port, udp.destination_port), (68, 67));
    }

    #[test]
    fn rejects_bad_lengths_checksums_and_fragments() {
        assert_eq!(EthernetFrame::parse(&[0; 13]), Err(NetworkError::Truncated));
        let mut ip = [0u8; 20];
        ip[0] = 0x45;
        ip[2..4].copy_from_slice(&20u16.to_be_bytes());
        ip[6..8].copy_from_slice(&0x2000u16.to_be_bytes());
        let sum = checksum(&ip);
        ip[10..12].copy_from_slice(&sum.to_be_bytes());
        assert_eq!(Ipv4Packet::parse(&ip), Err(NetworkError::Fragmented));
        ip[6..8].fill(0);
        ip[10] ^= 1;
        assert_eq!(Ipv4Packet::parse(&ip), Err(NetworkError::InvalidChecksum));
    }

    #[test]
    fn parses_dhcp_options_and_rejects_duplicate_or_truncated_values() {
        let mut bytes = [0u8; 252];
        bytes[236..240].copy_from_slice(&[99, 130, 83, 99]);
        bytes[240..247].copy_from_slice(&[53, 1, 1, 51, 4, 0, 0]);
        bytes[247..252].copy_from_slice(&[0, 0, 0, 0, 255]);
        let options = DhcpOptions::parse(&bytes).unwrap();
        assert_eq!(options.message_type, Some(1));
        assert_eq!(
            DhcpOptions::parse(&bytes[..244]),
            Err(NetworkError::Truncated)
        );
    }

    #[test]
    fn socket_table_binds_ports_with_process_and_protocol_isolation() {
        let mut table = SocketTable::<8>::new();
        let tcp = table.create(10, 1000, 2, 1, 0).unwrap();
        assert_eq!(tcp, 3);
        assert_eq!(table.bind(10, 1000, tcp, [0; 4], 8080), Ok(()));
        assert_eq!(table.listen(10, tcp, 10), Ok(()));
        assert_eq!(table.info(10, tcp).unwrap().state, SocketState::Listening);

        let udp = table.create(10, 1000, 2, 2, 0).unwrap();
        assert_eq!(table.bind(10, 1000, udp, [0; 4], 8080), Ok(()));
        let other = table.create(11, 1000, 2, 1, 6).unwrap();
        assert_eq!(
            table.bind(11, 1000, other, [127, 0, 0, 1], 8080),
            Err(NetworkError::AddressInUse)
        );
        assert_eq!(
            table.bind(11, 1000, other, [127, 0, 0, 1], 80),
            Err(NetworkError::PermissionDenied)
        );
        assert_eq!(table.close(10, tcp), Ok(()));
        assert_eq!(table.info(10, tcp), None);
    }
}

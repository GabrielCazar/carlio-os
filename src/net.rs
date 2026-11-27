//! Quadros Ethernet, ARP, IPv4, TCP, UDP, opções DHCP e descritores de sockets.

pub const ETHERNET_HEADER_LEN: usize = 14;
pub const IPV4_MIN_HEADER_LEN: usize = 20;
pub const UDP_HEADER_LEN: usize = 8;
pub const STATUS_PORT: u16 = 7704;

/// Resposta TCP para uma porta local sem listener. Ignora RST e tráfego fora da LAN.
pub fn tcp_closed_port_reply(
    frame: &[u8],
    mac: [u8; 6],
    address: [u8; 4],
    mask: [u8; 4],
    is_listening: impl FnOnce(u16) -> bool,
    output: &mut [u8; 60],
) -> Option<usize> {
    const FIN: u8 = 0x01;
    const SYN: u8 = 0x02;
    const RST: u8 = 0x04;
    const ACK: u8 = 0x10;

    let (peer_mac, peer_ip, request) = local_tcp(frame, mac, address)?;
    ipv4_next_hop(address, Some(mask), None, peer_ip)?;
    if request.flags & RST != 0 || is_listening(request.destination_port) {
        return None;
    }
    let (sequence, acknowledgement, flags) = if request.flags & ACK != 0 {
        (request.acknowledgement, 0, RST)
    } else {
        let length = request.payload.len() as u32
            + u32::from(request.flags & SYN != 0)
            + u32::from(request.flags & FIN != 0);
        (0, request.sequence.wrapping_add(length), RST | ACK)
    };
    let response = TcpSegment {
        source_port: request.destination_port,
        destination_port: request.source_port,
        sequence,
        acknowledgement,
        flags,
        window: 0,
        urgent_pointer: 0,
        options: &[],
        payload: &[],
    };
    tcp_frame(mac, peer_mac, address, peer_ip, &response, output)
}

fn local_tcp(
    frame: &[u8],
    mac: [u8; 6],
    address: [u8; 4],
) -> Option<([u8; 6], [u8; 4], TcpSegment<'_>)> {
    let ethernet = EthernetFrame::parse(frame).ok()?;
    if ethernet.ethertype != 0x0800
        || ethernet.destination != mac
        || ethernet.source == [0; 6]
        || ethernet.source == mac
        || ethernet.source[0] & 1 != 0
    {
        return None;
    }
    let ip = Ipv4Packet::parse(ethernet.payload).ok()?;
    if ip.destination != address || ip.ttl == 0 {
        return None;
    }
    Some((ethernet.source, ip.source, TcpSegment::parse(&ip).ok()?))
}

fn tcp_frame(
    mac: [u8; 6],
    peer_mac: [u8; 6],
    address: [u8; 4],
    peer_ip: [u8; 4],
    segment: &TcpSegment<'_>,
    output: &mut [u8],
) -> Option<usize> {
    let length = 14 + encode_ipv4_tcp(address, peer_ip, segment, output.get_mut(14..)?).ok()?;
    let padded = length.max(60);
    output.get_mut(length..padded)?.fill(0);
    output[..6].copy_from_slice(&peer_mac);
    output[6..12].copy_from_slice(&mac);
    output[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
    Some(padded)
}

/// Bounded, opt-in LAN health check. Replies cannot exceed the request size.
pub fn status_reply(
    frame: &[u8],
    mac: [u8; 6],
    address: [u8; 4],
    mask: [u8; 4],
    output: &mut [u8; 60],
) -> Option<usize> {
    let ethernet = EthernetFrame::parse(frame).ok()?;
    if ethernet.source == [0; 6] || ethernet.source == mac || ethernet.source[0] & 1 != 0 {
        return None;
    }
    output.fill(0);
    output[..6].copy_from_slice(&ethernet.source);
    output[6..12].copy_from_slice(&mac);
    output[12..14].copy_from_slice(&ethernet.ethertype.to_be_bytes());
    match ethernet.ethertype {
        0x0806 if ethernet.destination == [0xff; 6] || ethernet.destination == mac => {
            let arp = ArpPacket::parse(ethernet.payload).ok()?;
            if arp.operation != 1 || arp.target_ipv4 != address || arp.sender_mac != ethernet.source
            {
                return None;
            }
            output[14..22].copy_from_slice(&[0, 1, 8, 0, 6, 4, 0, 2]);
            output[22..28].copy_from_slice(&mac);
            output[28..32].copy_from_slice(&address);
            output[32..38].copy_from_slice(&arp.sender_mac);
            output[38..42].copy_from_slice(&arp.sender_ipv4);
        }
        0x0800 if ethernet.destination == mac => {
            let ip = Ipv4Packet::parse(ethernet.payload).ok()?;
            let mask = u32::from_be_bytes(mask);
            let host_mask = !mask;
            let address_bits = u32::from_be_bytes(address);
            let source_bits = u32::from_be_bytes(ip.source);
            if ip.destination != address
                || ip.protocol != 17
                || ip.ttl == 0
                || ip.source[0] == 0
                || ip.source[0] == 127
                || ip.source[0] >= 224
                || mask == 0
                || host_mask & host_mask.wrapping_add(1) != 0
                || source_bits & mask != address_bits & mask
                || source_bits == address_bits
                || source_bits & host_mask == 0
                || source_bits & host_mask == host_mask
            {
                return None;
            }
            let udp = UdpDatagram::parse(ip.payload, ip.source, ip.destination).ok()?;
            if udp.destination_port != STATUS_PORT || udp.payload != b"STATUS" {
                return None;
            }
            encode_ipv4_udp(
                address,
                ip.source,
                STATUS_PORT,
                udp.source_port,
                b"OK",
                &mut output[14..],
            )
            .ok()?;
        }
        _ => return None,
    }
    Some(output.len())
}

/// Monta uma consulta recursiva A/IN para um nome ASCII.
pub fn dns_a_query(name: &str, id: u16, output: &mut [u8; 271]) -> Result<usize, NetworkError> {
    let name = name.strip_suffix('.').unwrap_or(name);
    if name.len() > 253 {
        return Err(NetworkError::InvalidLength);
    }
    output[..12].fill(0);
    output[..2].copy_from_slice(&id.to_be_bytes());
    output[2] = 1;
    output[5] = 1;
    let mut cursor = 12;
    for label in name.split('.') {
        if label.is_empty()
            || label.len() > 63
            || !label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(NetworkError::InvalidAddress);
        }
        output[cursor] = label.len() as u8;
        cursor += 1;
        output[cursor..cursor + label.len()].copy_from_slice(label.as_bytes());
        cursor += label.len();
    }
    output[cursor..cursor + 5].copy_from_slice(&[0, 0, 1, 0, 1]);
    Ok(cursor + 5)
}

/// Extrai um A/IN do nome consultado ou de uma cadeia CNAME/IN na resposta.
pub fn dns_a_answer(query: &[u8], reply: &[u8]) -> Option<[u8; 4]> {
    if query.len() < 17
        || reply.len() < query.len()
        || reply[..2] != query[..2]
        || reply[2] & 0xfa != 0x80
        || reply[3] & 0x0f != 0
        || reply[4..6] != [0, 1]
        || !reply[12..query.len()].eq_ignore_ascii_case(&query[12..])
    {
        return None;
    }
    let original = &query[12..query.len() - 4];
    let mut target = [0; 255];
    target.get_mut(..original.len())?.copy_from_slice(original);
    let mut target_len = original.len();
    let answers = u16::from_be_bytes([reply[6], reply[7]]);
    for _ in 0..=8 {
        let mut alias = None;
        let mut cursor = query.len();
        for _ in 0..answers {
            let mut owner = [0; 255];
            let (next, owner_len) = dns_name(reply, cursor, &mut owner)?;
            let header = reply.get(next..next + 10)?;
            let length = usize::from(u16::from_be_bytes([header[8], header[9]]));
            let data_start = next + 10;
            cursor = data_start + length;
            let data = reply.get(data_start..cursor)?;
            if !owner[..owner_len].eq_ignore_ascii_case(&target[..target_len]) {
                continue;
            }
            match header[..4] {
                [0, 1, 0, 1] if length == 4 => return data.try_into().ok(),
                [0, 5, 0, 1] => alias = Some((data_start, cursor)),
                _ => {}
            }
        }
        let (start, end) = alias?;
        let (next, length) = dns_name(reply, start, &mut target)?;
        if next != end {
            return None;
        }
        target_len = length;
    }
    None
}

fn dns_name(packet: &[u8], mut cursor: usize, name: &mut [u8; 255]) -> Option<(usize, usize)> {
    let mut next = None;
    let mut length = 0;
    for _ in 0..255 {
        let label = *packet.get(cursor)?;
        if label & 0xc0 == 0xc0 {
            let target = (usize::from(label & 0x3f) << 8) | usize::from(*packet.get(cursor + 1)?);
            if target < 12 || target >= cursor {
                return None;
            }
            next.get_or_insert(cursor + 2);
            cursor = target;
            continue;
        }
        if label > 63 {
            return None;
        }
        let end = cursor + 1 + usize::from(label);
        let label = packet.get(cursor..end)?;
        name.get_mut(length..length + label.len())?
            .copy_from_slice(label);
        length += label.len();
        if label == [0] {
            return Some((next.unwrap_or(end), length));
        }
        cursor = end;
    }
    None
}

/// Seleciona o destino ARP para uma rota unicast diretamente conectada
/// ou para a rota padrão de uma interface IPv4.
pub fn ipv4_next_hop(
    source: [u8; 4],
    mask: Option<[u8; 4]>,
    gateway: Option<[u8; 4]>,
    destination: [u8; 4],
) -> Option<[u8; 4]> {
    fn unicast(address: [u8; 4]) -> bool {
        (1..224).contains(&address[0]) && address[0] != 127
    }

    if !unicast(source) || !unicast(destination) || source == destination {
        return None;
    }
    let mask = u32::from_be_bytes(mask?);
    if mask == 0 || mask.leading_ones() + mask.trailing_zeros() != 32 {
        return None;
    }
    let network = u32::from_be_bytes(source) & mask;
    let on_link = |address| {
        let address = u32::from_be_bytes(address);
        address & mask == network
            && (mask.count_zeros() <= 1 || (address != network && address != (network | !mask)))
    };
    if u32::from_be_bytes(destination) & mask == network {
        return on_link(destination).then_some(destination);
    }
    gateway.filter(|address| unicast(*address) && *address != source && on_link(*address))
}

/// Escreve um datagrama UDP com checksum e cabeçalho IPv4 sem fragmentação.
pub fn encode_ipv4_udp(
    source: [u8; 4],
    destination: [u8; 4],
    source_port: u16,
    destination_port: u16,
    payload: &[u8],
    output: &mut [u8],
) -> Result<usize, NetworkError> {
    if payload.len() > 1500 - IPV4_MIN_HEADER_LEN - UDP_HEADER_LEN {
        return Err(NetworkError::InvalidLength);
    }
    let total = IPV4_MIN_HEADER_LEN + UDP_HEADER_LEN + payload.len();
    if total > output.len() {
        return Err(NetworkError::InvalidLength);
    }
    if source_port == 0 || destination_port == 0 {
        return Err(NetworkError::InvalidPort);
    }
    let (ip, udp) = output[..total].split_at_mut(IPV4_MIN_HEADER_LEN);
    encode_ipv4_header(source, destination, 17, total, ip);
    let udp_len = udp.len() as u16;
    udp[..2].copy_from_slice(&source_port.to_be_bytes());
    udp[2..4].copy_from_slice(&destination_port.to_be_bytes());
    udp[4..6].copy_from_slice(&udp_len.to_be_bytes());
    udp[6..8].fill(0);
    udp[8..].copy_from_slice(payload);
    let transport_checksum = ipv4_transport_checksum(source, destination, 17, udp);
    let transport_checksum = if transport_checksum == 0 {
        u16::MAX
    } else {
        transport_checksum
    };
    udp[6..8].copy_from_slice(&transport_checksum.to_be_bytes());
    Ok(total)
}

const TCP_MIN_HEADER_LEN: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TcpSegment<'a> {
    pub source_port: u16,
    pub destination_port: u16,
    pub sequence: u32,
    pub acknowledgement: u32,
    pub flags: u8,
    pub window: u16,
    pub urgent_pointer: u16,
    pub options: &'a [u8],
    pub payload: &'a [u8],
}

impl<'a> TcpSegment<'a> {
    pub fn parse(ip: &Ipv4Packet<'a>) -> Result<Self, NetworkError> {
        if ip.protocol != 6 {
            return Err(NetworkError::Unsupported);
        }
        let bytes = ip.payload;
        if bytes.len() < TCP_MIN_HEADER_LEN {
            return Err(NetworkError::Truncated);
        }
        let header_len = usize::from(bytes[12] >> 4) * 4;
        if header_len < TCP_MIN_HEADER_LEN || header_len > bytes.len() {
            return Err(NetworkError::InvalidLength);
        }
        if ipv4_transport_checksum(ip.source, ip.destination, 6, bytes) != 0 {
            return Err(NetworkError::InvalidChecksum);
        }
        let source_port = u16be(bytes, 0);
        let destination_port = u16be(bytes, 2);
        if source_port == 0 || destination_port == 0 {
            return Err(NetworkError::InvalidPort);
        }
        Ok(Self {
            source_port,
            destination_port,
            sequence: u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            acknowledgement: u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            flags: bytes[13],
            window: u16be(bytes, 14),
            urgent_pointer: u16be(bytes, 18),
            options: &bytes[TCP_MIN_HEADER_LEN..header_len],
            payload: &bytes[header_len..],
        })
    }
}

pub fn encode_ipv4_tcp(
    source: [u8; 4],
    destination: [u8; 4],
    segment: &TcpSegment<'_>,
    output: &mut [u8],
) -> Result<usize, NetworkError> {
    if segment.source_port == 0 || segment.destination_port == 0 {
        return Err(NetworkError::InvalidPort);
    }
    if segment.options.len() > 40 || !segment.options.len().is_multiple_of(4) {
        return Err(NetworkError::InvalidOption);
    }
    let header_len = TCP_MIN_HEADER_LEN + segment.options.len();
    let total = IPV4_MIN_HEADER_LEN + header_len;
    if segment.payload.len() > 1500 - total {
        return Err(NetworkError::InvalidLength);
    }
    let total = total + segment.payload.len();
    if total > output.len() {
        return Err(NetworkError::InvalidLength);
    }
    let (ip, tcp) = output[..total].split_at_mut(IPV4_MIN_HEADER_LEN);
    encode_ipv4_header(source, destination, 6, total, ip);
    tcp[..TCP_MIN_HEADER_LEN].fill(0);
    tcp[..2].copy_from_slice(&segment.source_port.to_be_bytes());
    tcp[2..4].copy_from_slice(&segment.destination_port.to_be_bytes());
    tcp[4..8].copy_from_slice(&segment.sequence.to_be_bytes());
    tcp[8..12].copy_from_slice(&segment.acknowledgement.to_be_bytes());
    tcp[12] = ((header_len / 4) as u8) << 4;
    tcp[13] = segment.flags;
    tcp[14..16].copy_from_slice(&segment.window.to_be_bytes());
    tcp[18..20].copy_from_slice(&segment.urgent_pointer.to_be_bytes());
    tcp[TCP_MIN_HEADER_LEN..header_len].copy_from_slice(segment.options);
    tcp[header_len..].copy_from_slice(segment.payload);
    let tcp_checksum = ipv4_transport_checksum(source, destination, 6, tcp);
    tcp[16..18].copy_from_slice(&tcp_checksum.to_be_bytes());
    Ok(total)
}

fn encode_ipv4_header(
    source: [u8; 4],
    destination: [u8; 4],
    protocol: u8,
    total: usize,
    ip: &mut [u8],
) {
    ip.fill(0);
    ip[0] = 0x45;
    ip[2..4].copy_from_slice(&(total as u16).to_be_bytes());
    ip[6] = 0x40;
    ip[8] = 64;
    ip[9] = protocol;
    ip[12..16].copy_from_slice(&source);
    ip[16..20].copy_from_slice(&destination);
    let header_checksum = checksum(ip);
    ip[10..12].copy_from_slice(&header_checksum.to_be_bytes());
}

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
    PortsExhausted,
    PermissionDenied,
    BadDescriptor,
    UnsupportedSocket,
    InvalidPort,
    InvalidState,
    WouldBlock,
    ConnectionReset,
    BrokenPipe,
    NotConnected,
    ConnectionRefused,
    ConnectionTimedOut,
    InProgress,
    AlreadyConnected,
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
    Connecting,
    Connected,
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
    pub nonblocking: bool,
}

const SOCK_NONBLOCK: u64 = 0x800;
const SOCK_CLOEXEC: u64 = 0x80000;
const EPHEMERAL_PORTS: core::ops::RangeInclusive<u16> = 49152..=65535;

#[derive(Clone, Copy)]
struct SocketEntry {
    info: SocketInfo,
    occupied: bool,
}

#[derive(Clone, Copy)]
struct SocketDescriptor {
    owner: u64,
    number: u32,
    endpoint: usize,
    close_on_exec: bool,
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
            nonblocking: false,
        },
        occupied: false,
    };
}

#[path = "net_tcp.rs"]
mod tcp;
use tcp::Connection;

/// Tabela limitada de endpoints IPv4 e conexões por processo.
pub struct SocketTable<const N: usize> {
    entries: [SocketEntry; N],
    descriptors: [Option<SocketDescriptor>; N],
    connections: [Connection; 16],
}
impl<const N: usize> SocketTable<N> {
    pub const fn new() -> Self {
        Self {
            entries: [SocketEntry::EMPTY; N],
            descriptors: [None; N],
            connections: [Connection::EMPTY; 16],
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
        self.create_excluding(process_id, uid, domain, kind, protocol, |_| false)
    }

    pub fn create_excluding(
        &mut self,
        process_id: u64,
        uid: u32,
        domain: u64,
        kind: u64,
        protocol: u64,
        reserved: impl Fn(u32) -> bool,
    ) -> Result<u32, NetworkError> {
        if domain != 2 {
            return Err(NetworkError::UnsupportedSocket);
        }
        let transport = match (kind & !(SOCK_NONBLOCK | SOCK_CLOEXEC), protocol) {
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
        let descriptor_slot = self.descriptors.iter().position(Option::is_none)
            .ok_or(NetworkError::InvalidState)?;
        let descriptor = (3..3 + N as u32)
            .find(|&fd| !reserved(fd) && self.descriptor_slot(process_id, fd).is_none())
            .ok_or(NetworkError::InvalidState)?;
        self.descriptors[descriptor_slot] = Some(SocketDescriptor {
            owner: process_id, number: descriptor, endpoint: slot,
            close_on_exec: kind & SOCK_CLOEXEC != 0,
        });
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
                nonblocking: kind & SOCK_NONBLOCK != 0,
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
        let index = self
            .find(process_id, descriptor)
            .ok_or(NetworkError::BadDescriptor)?;
        if self.entries[index].info.state != SocketState::Created {
            return Err(NetworkError::InvalidState);
        }
        if port != 0 && port < 1024 && uid != 0 {
            return Err(NetworkError::PermissionDenied);
        }
        let candidate = self.entries[index].info;
        let port_in_use = |port| {
            self.entries.iter().any(|entry| {
                entry.occupied
                    && entry.info.protocol == candidate.protocol
                    && entry.info.local_port == port
                    && (address == [0; 4]
                        || entry.info.local_address == [0; 4]
                        || entry.info.local_address == address)
            })
        };
        let port = if port == 0 {
            let mut ports = EPHEMERAL_PORTS;
            ports
                .find(|port| {
                    !port_in_use(*port)
                        && (candidate.protocol != SocketProtocol::Tcp
                            || !self.tcp_closing_port_in_use(address, *port))
                })
                .ok_or(NetworkError::PortsExhausted)?
        } else if port_in_use(port) {
            return Err(NetworkError::AddressInUse);
        } else {
            port
        };
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

    pub fn inherit(&mut self, parent: u64, child: u64) -> Result<(), NetworkError> {
        if parent == child || self.descriptors.iter().flatten().any(|entry| entry.owner == child) {
            return Err(NetworkError::InvalidState);
        }
        let count = self.descriptors.iter().flatten().filter(|entry| entry.owner == parent).count();
        if self.descriptors.iter().filter(|entry| entry.is_none()).count() < count {
            return Err(NetworkError::InvalidState);
        }
        for slot in 0..N {
            let Some(mut entry) = self.descriptors[slot].filter(|entry| entry.owner == parent) else {
                continue;
            };
            let target = self.descriptors.iter().position(Option::is_none).unwrap();
            entry.owner = child;
            self.descriptors[target] = Some(entry);
        }
        Ok(())
    }

    pub fn duplicate_excluding(
        &mut self,
        owner: u64,
        fd: u32,
        minimum: u32,
        close_on_exec: bool,
        reserved: impl Fn(u32) -> bool,
    ) -> Result<u32, NetworkError> {
        self.descriptor_slot(owner, fd).ok_or(NetworkError::BadDescriptor)?;
        let target = (minimum..3 + N as u32)
            .find(|&number| !reserved(number) && self.descriptor_slot(owner, number).is_none())
            .ok_or(NetworkError::InvalidState)?;
        self.duplicate_to(owner, fd, target, close_on_exec)
    }

    pub fn duplicate_to(
        &mut self,
        process_id: u64,
        fd: u32,
        target: u32,
        close_on_exec: bool,
    ) -> Result<u32, NetworkError> {
        let source = self.descriptor_slot(process_id, fd).ok_or(NetworkError::BadDescriptor)?;
        if fd == target {
            return Ok(fd);
        }
        let target_slot = self.descriptor_slot(process_id, target)
            .or_else(|| self.descriptors.iter().position(Option::is_none))
            .ok_or(NetworkError::InvalidState)?;
        let mut alias = self.descriptors[source].ok_or(NetworkError::BadDescriptor)?;
        self.close_slot(target_slot);
        alias.number = target;
        alias.close_on_exec = close_on_exec;
        self.descriptors[target_slot] = Some(alias);
        Ok(target)
    }

    fn close_slot(&mut self, slot: usize) {
        if let Some(entry) = self.descriptors[slot].take() {
            if !self.descriptors.iter().flatten().any(|other| other.endpoint == entry.endpoint) {
                self.release_connections(entry.endpoint as u32 + 1);
                self.entries[entry.endpoint] = SocketEntry::EMPTY;
            }
        }
    }

    pub fn close(&mut self, process_id: u64, descriptor: u32) -> Result<(), NetworkError> {
        let slot = self.descriptor_slot(process_id, descriptor).ok_or(NetworkError::BadDescriptor)?;
        self.close_slot(slot);
        Ok(())
    }

    pub fn close_process(&mut self, process_id: u64) -> usize {
        let mut closed = 0;
        for slot in 0..N {
            if self.descriptors[slot].is_some_and(|entry| entry.owner == process_id) {
                self.close_slot(slot);
                closed += 1;
            }
        }
        closed
    }

    pub fn fcntl(
        &mut self,
        process_id: u64,
        fd: u32,
        operation: crate::linux_abi::DescriptorControl,
    ) -> Result<u64, NetworkError> {
        let slot = self.descriptor_slot(process_id, fd).ok_or(NetworkError::BadDescriptor)?;
        let descriptor = self.descriptors[slot].as_mut().ok_or(NetworkError::BadDescriptor)?;
        Ok(operation.apply(2, &mut descriptor.close_on_exec,
            &mut self.entries[descriptor.endpoint].info.nonblocking))
    }

    pub fn close_on_exec(&mut self, process_id: u64) {
        for slot in 0..N {
            if self.descriptors[slot].is_some_and(|entry| entry.owner == process_id && entry.close_on_exec) {
                self.close_slot(slot);
            }
        }
    }

    pub fn info(&self, process_id: u64, descriptor: u32) -> Option<SocketInfo> {
        let index = self.find(process_id, descriptor)?;
        Some(SocketInfo { process_id, descriptor, ..self.entries[index].info })
    }

    pub fn is_listening(&self, address: [u8; 4], port: u16) -> bool {
        self.entries.iter().any(|entry| {
            entry.occupied
                && entry.info.state == SocketState::Listening
                && entry.info.local_port == port
                && (entry.info.local_address == [0; 4] || entry.info.local_address == address)
        })
    }

    fn descriptor_slot(&self, process_id: u64, descriptor: u32) -> Option<usize> {
        self.descriptors.iter().position(|entry| entry.is_some_and(|entry| {
            entry.owner == process_id && entry.number == descriptor
        }))
    }

    fn find(&self, process_id: u64, descriptor: u32) -> Option<usize> {
        Some(self.descriptors[self.descriptor_slot(process_id, descriptor)?]?.endpoint)
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
        if checksum_field != 0
            && ipv4_transport_checksum(source, destination, 17, &bytes[..length]) != 0
        {
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

fn ipv4_transport_checksum(
    source: [u8; 4],
    destination: [u8; 4],
    protocol: u8,
    transport: &[u8],
) -> u16 {
    let mut sum = 0u32;
    for address in [source, destination] {
        sum += u16::from_be_bytes([address[0], address[1]]) as u32;
        sum += u16::from_be_bytes([address[2], address[3]]) as u32;
    }
    sum += u32::from(protocol);
    sum += transport.len() as u32;
    let (chunks, remainder) = transport.as_chunks::<2>();
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
    #[test]
    fn selects_direct_and_default_ipv4_routes() {
        let source = [192, 168, 3, 20];
        let mask = Some([255, 255, 255, 0]);
        let gateway = Some([192, 168, 3, 1]);
        let peer = [192, 168, 3, 53];
        let remote = [203, 0, 113, 53];
        assert_eq!(ipv4_next_hop(source, mask, gateway, peer), Some(peer));
        assert_eq!(ipv4_next_hop(source, mask, gateway, remote), gateway);
        assert_eq!(ipv4_next_hop(source, mask, None, remote), None);
        assert_eq!(
            ipv4_next_hop(source, mask, gateway, [192, 168, 3, 255]),
            None
        );
        assert_eq!(
            ipv4_next_hop(source, Some([255, 0, 255, 0]), gateway, peer),
            None
        );
        assert_eq!(
            ipv4_next_hop(source, mask, Some([192, 168, 4, 1]), remote),
            None
        );
    }

    #[test]
    fn dns_checks_transaction_question_and_compressed_owner() {
        let mut query = [0; 271];
        let length = dns_a_query("example.test.", 0x1234, &mut query).unwrap();
        let query = &query[..length];
        let mut reply = [0; 64];
        reply[..length].copy_from_slice(query);
        reply[2] = 0x81;
        reply[3] = 0x80;
        reply[7] = 1;
        let answer = [0xc0, 12, 0, 1, 0, 1, 0, 0, 0, 30, 0, 4, 192, 0, 2, 1];
        let reply = &mut reply[..length + answer.len()];
        reply[length..].copy_from_slice(&answer);
        assert_eq!(dns_a_answer(query, reply), Some([192, 0, 2, 1]));
        assert_eq!(dns_a_answer(query, &reply[..reply.len() - 1]), None);
        reply[1] ^= 1;
        assert_eq!(dns_a_answer(query, reply), None);
        reply[1] ^= 1;
        reply[length + 1] = length as u8;
        assert_eq!(dns_a_answer(query, reply), None);
    }

    #[test]
    fn dns_follows_compressed_alias_even_when_its_address_appears_first() {
        let mut query = [0; 271];
        let length = dns_a_query("example.test", 42, &mut query).unwrap();
        let query = &query[..length];
        let mut reply = query.to_vec();
        reply[2..4].copy_from_slice(&[0x81, 0x80]);
        reply[7] = 2;
        reply.extend_from_slice(&[
            3, b'w', b'w', b'w', 0xc0, 20, 0, 1, 0, 1, 0, 0, 0, 30, 0, 4, 192, 0, 2, 1,
        ]);
        reply.extend_from_slice(&[0xc0, 12, 0, 5, 0, 1, 0, 0, 0, 30, 0, 2, 0xc0, length as u8]);
        assert_eq!(dns_a_answer(query, &reply), Some([192, 0, 2, 1]));
        *reply.last_mut().unwrap() = 12;
        assert_eq!(dns_a_answer(query, &reply), None);
    }

    #[test]
    fn encodes_ipv4_udp_and_detects_corrupted_payload() {
        let mut bytes = [0; 64];
        let source = [192, 0, 2, 10];
        let destination = [203, 0, 113, 53];
        let len = encode_ipv4_udp(source, destination, 49152, 53, b"odd", &mut bytes).unwrap();
        let ip = Ipv4Packet::parse(&bytes[..len]).unwrap();
        assert_eq!(
            (ip.source, ip.destination, ip.protocol),
            (source, destination, 17)
        );
        let udp = UdpDatagram::parse(ip.payload, source, destination).unwrap();
        assert_eq!(
            (udp.source_port, udp.destination_port, udp.payload),
            (49152, 53, &b"odd"[..])
        );
        bytes[len - 1] ^= 1;
        let ip = Ipv4Packet::parse(&bytes[..len]).unwrap();
        assert_eq!(
            UdpDatagram::parse(ip.payload, source, destination),
            Err(NetworkError::InvalidChecksum)
        );
    }

    use super::*;

    #[test]
    fn tcp_closed_port_resets_only_local_unicast_without_a_listener() {
        let mac = [2, 0, 0, 0, 0, 1];
        let peer = [2, 0, 0, 0, 0, 2];
        let address = [192, 0, 2, 1];
        let source = [192, 0, 2, 2];
        let mask = [255, 255, 255, 0];
        let mut request = TcpSegment {
            source_port: 49152,
            destination_port: 389,
            sequence: u32::MAX,
            acknowledgement: 42,
            flags: 0x02,
            window: 4096,
            urgent_pointer: 0,
            options: &[],
            payload: &[],
        };
        let mut frame = [0; 60];
        frame[..6].copy_from_slice(&mac);
        frame[6..12].copy_from_slice(&peer);
        frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
        let mut reply = [0; 60];
        let mut table = SocketTable::<2>::new();
        let socket = table.create(8, 0, 2, 1, 0).unwrap();
        table.bind(8, 0, socket, [0; 4], 389).unwrap();
        encode_ipv4_tcp(source, address, &request, &mut frame[14..]).unwrap();
        assert_eq!(
            tcp_closed_port_reply(
                &frame,
                mac,
                address,
                mask,
                |p| table.is_listening(address, p),
                &mut reply
            ),
            Some(60)
        );
        assert_eq!(&reply[..6], &peer);
        let ip = Ipv4Packet::parse(&reply[14..]).unwrap();
        assert_eq!(ip.destination, source);
        let reset = TcpSegment::parse(&ip).unwrap();
        assert_eq!((reset.source_port, reset.destination_port), (389, 49152));
        assert_eq!(
            (reset.flags, reset.sequence, reset.acknowledgement),
            (0x14, 0, 0)
        );

        table.listen(8, socket, 2).unwrap();
        assert_eq!(
            tcp_closed_port_reply(
                &frame,
                mac,
                address,
                mask,
                |p| table.is_listening(address, p),
                &mut reply
            ),
            None
        );
        request.flags = 0x10;
        encode_ipv4_tcp(source, address, &request, &mut frame[14..]).unwrap();
        assert_eq!(
            tcp_closed_port_reply(&frame, mac, address, mask, |_| false, &mut reply),
            Some(60)
        );
        let reset = TcpSegment::parse(&Ipv4Packet::parse(&reply[14..]).unwrap()).unwrap();
        assert_eq!(
            (reset.flags, reset.sequence, reset.acknowledgement),
            (0x04, 42, 0)
        );
        request.flags = 0x04;
        encode_ipv4_tcp(source, address, &request, &mut frame[14..]).unwrap();
        assert_eq!(
            tcp_closed_port_reply(&frame, mac, address, mask, |_| false, &mut reply),
            None
        );
        request.flags = 0x02;
        encode_ipv4_tcp([192, 0, 3, 2], address, &request, &mut frame[14..]).unwrap();
        assert_eq!(
            tcp_closed_port_reply(&frame, mac, address, mask, |_| false, &mut reply),
            None
        );
    }

    #[test]
    fn tcp_ipv4_round_trip_and_rejects_corruption() {
        let source = [192, 0, 2, 20];
        let destination = [198, 51, 100, 9];
        let segment = TcpSegment {
            source_port: 49152,
            destination_port: 389,
            sequence: 0x1234_5678,
            acknowledgement: 0x8765_4321,
            flags: 0x18,
            window: 4096,
            urgent_pointer: 0,
            options: &[1, 1, 1, 0],
            payload: b"abc",
        };
        let mut packet = [0u8; 64];
        let length = encode_ipv4_tcp(source, destination, &segment, &mut packet).unwrap();
        assert_eq!(length, 47);
        assert_eq!(&packet[36..38], &[0xe8, 0x57]);
        let ip = Ipv4Packet::parse(&packet[..length]).unwrap();
        assert_eq!(TcpSegment::parse(&ip), Ok(segment));

        packet[length - 1] ^= 1;
        let ip = Ipv4Packet::parse(&packet[..length]).unwrap();
        assert_eq!(TcpSegment::parse(&ip), Err(NetworkError::InvalidChecksum));
        packet[32] = 0xf0;
        let ip = Ipv4Packet::parse(&packet[..length]).unwrap();
        assert_eq!(TcpSegment::parse(&ip), Err(NetworkError::InvalidLength));
        assert_eq!(
            encode_ipv4_tcp(source, destination, &segment, &mut packet[..length - 1]),
            Err(NetworkError::InvalidLength)
        );
    }

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

    #[test]
    fn socket_table_assigns_ephemeral_ports_and_releases_close_on_exec() {
        let mut table = SocketTable::<4>::new();
        let first = table
            .create(10, 1000, 2, 1 | SOCK_CLOEXEC | SOCK_NONBLOCK, 0)
            .unwrap();
        let second = table.create(11, 1000, 2, 1, 0).unwrap();
        table.bind(10, 1000, first, [0; 4], 0).unwrap();
        table.bind(11, 1000, second, [127, 0, 0, 1], 0).unwrap();
        let first_info = table.info(10, first).unwrap();
        let second_info = table.info(11, second).unwrap();
        assert!(first_info.nonblocking);
        assert_ne!(first_info.local_port, second_info.local_port);
        table.close_on_exec(11);
        assert_eq!(table.info(11, second), Some(second_info));
        table.close_on_exec(10);
        assert_eq!(table.info(10, first), None);
        let replacement = table.create(10, 1000, 2, 1, 0).unwrap();
        table
            .bind(10, 1000, replacement, [0; 4], first_info.local_port)
            .unwrap();
    }
}

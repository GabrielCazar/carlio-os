use super::{
    NetworkError, SocketProtocol, SocketState, SocketTable, TcpSegment, local_tcp, tcp_frame,
};

const FIN: u8 = 1;
const SYN: u8 = 2;
const RST: u8 = 4;
const ACK: u8 = 16;
const MSS: usize = 536;
// The kernel supplies PIT ticks at 100 Hz.
const RETRY_TICKS: u64 = 100;
const CONNECTION_TIMEOUT: u64 = 3000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Free,
    Handshake,
    SynSent,
    Open,
    Reset,
    Refused,
    TimedOut,
}

#[derive(Clone, Copy)]
pub(super) struct Connection {
    state: State,
    listener: u32,
    fd: u32,
    local: [u8; 4],
    peer: [u8; 4],
    peer_mac: [u8; 6],
    local_port: u16,
    peer_port: u16,
    next_rx: u32,
    next_tx: u32,
    peer_window: u16,
    rx: [u8; 1460],
    rx_len: usize,
    tx: [u8; MSS],
    tx_len: usize,
    peer_fin: bool,
    closing: bool,
    read_shutdown: bool,
    write_shutdown: bool,
    ack_pending: bool,
    fin_acked: bool,
    sent: bool,
    last_send: u64,
    expires: u64,
}

impl Connection {
    pub(super) const EMPTY: Self = Self {
        state: State::Free,
        listener: 0,
        fd: 0,
        local: [0; 4],
        peer: [0; 4],
        peer_mac: [0; 6],
        local_port: 0,
        peer_port: 0,
        next_rx: 0,
        next_tx: 0,
        peer_window: 0,
        rx: [0; 1460],
        rx_len: 0,
        tx: [0; MSS],
        tx_len: 0,
        peer_fin: false,
        closing: false,
        read_shutdown: false,
        write_shutdown: false,
        ack_pending: false,
        fin_acked: false,
        sent: false,
        last_send: 0,
        expires: 0,
    };

    fn fail(&mut self) {
        if self.fd == 0 {
            self.state = State::Free;
        } else {
            self.state = State::Reset;
            self.rx_len = 0;
        }
    }

    fn receive(&mut self, segment: TcpSegment<'_>, now: u64) {
        if self.state == State::SynSent {
            if !self.sent || segment.acknowledgement != self.next_tx.wrapping_add(1) {
                return;
            }
            match segment.flags & (SYN | ACK | RST | FIN) {
                flags if flags == (RST | ACK) => self.state = State::Refused,
                flags if flags == (SYN | ACK) && segment.payload.is_empty() => {
                    self.next_tx = segment.acknowledgement;
                    self.next_rx = segment.sequence.wrapping_add(1);
                    self.peer_window = segment.window;
                    self.state = State::Open;
                    self.sent = false;
                    self.ack_pending = true;
                }
                _ => {}
            }
            return;
        }
        if segment.flags & RST != 0 {
            if segment.sequence == self.next_rx {
                self.fail();
            }
            return;
        }
        if self.state == State::Handshake {
            if self.sent
                && segment.flags & (SYN | ACK) == ACK
                && segment.sequence == self.next_rx
                && segment.acknowledgement == self.next_tx.wrapping_add(1)
            {
                self.next_tx = self.next_tx.wrapping_add(1);
                self.state = State::Open;
                self.sent = false;
                self.expires = now + CONNECTION_TIMEOUT;
            } else {
                return;
            }
        }
        if self.listener == 0
            && segment.flags & (SYN | ACK | FIN) == (SYN | ACK)
            && segment.sequence.wrapping_add(1) == self.next_rx
            && segment.acknowledgement == self.next_tx
            && segment.payload.is_empty()
        {
            self.ack_pending = true;
            return;
        }
        if segment.flags & ACK == 0 || segment.flags & SYN != 0 {
            return;
        }
        if segment.sequence != self.next_rx {
            self.ack_pending = true;
            return;
        }
        let in_flight = if self.sent {
            self.tx_len as u32 + u32::from(self.write_shutdown && self.tx_len == 0)
        } else {
            0
        };
        let acknowledged = segment.acknowledgement.wrapping_sub(self.next_tx);
        if acknowledged > in_flight {
            self.ack_pending = true;
            return;
        }
        self.peer_window = segment.window;
        if acknowledged != 0 {
            self.next_tx = segment.acknowledgement;
            if self.tx_len == 0 {
                self.fin_acked = true;
            } else {
                let consumed = acknowledged as usize;
                self.tx.copy_within(consumed..self.tx_len, 0);
                self.tx_len -= consumed;
                if self.tx_len != 0 {
                    self.expires = now + CONNECTION_TIMEOUT;
                }
            }
            self.sent = self.tx_len != 0;
        }
        if segment.payload.is_empty() && segment.flags & FIN == 0 {
            if self.closing && self.fin_acked && self.peer_fin && !self.ack_pending {
                self.state = State::Free;
            }
            return;
        }
        if !self.peer_fin && (self.read_shutdown || segment.payload.len() <= self.rx.len() - self.rx_len) {
            if !self.read_shutdown {
                let end = self.rx_len + segment.payload.len();
                self.rx[self.rx_len..end].copy_from_slice(segment.payload);
                self.rx_len = end;
            }
            self.peer_fin = segment.flags & FIN != 0;
            self.next_rx = self
                .next_rx
                .wrapping_add(segment.payload.len() as u32 + u32::from(self.peer_fin));
        }
        self.ack_pending = true;
    }

    fn transmit(&mut self, mac: [u8; 6], now: u64, send: &mut impl FnMut(&[u8]) -> bool) {
        if self.write_shutdown && self.expires == 0 {
            self.expires = now + CONNECTION_TIMEOUT;
        }
        let timed = matches!(self.state, State::Handshake | State::SynSent)
            || self.fd == 0
            || self.tx_len != 0
            || (self.write_shutdown && !self.fin_acked);
        if timed && now >= self.expires {
            if self.state == State::SynSent {
                self.state = State::TimedOut;
            } else {
                self.fail();
            }
            return;
        }
        let retry = !self.sent || now.saturating_sub(self.last_send) >= RETRY_TICKS;
        let (flags, payload) = if self.state == State::SynSent && retry {
            (SYN, &[][..])
        } else if self.state == State::Handshake && retry {
            (SYN | ACK, &[][..])
        } else if self.tx_len != 0 && retry {
            (ACK | 8, &self.tx[..self.tx_len])
        } else if self.write_shutdown && !self.fin_acked && self.tx_len == 0 && retry {
            (FIN | ACK, &[][..])
        } else if self.ack_pending {
            (ACK, &[][..])
        } else {
            return;
        };
        let segment = TcpSegment {
            source_port: self.local_port,
            destination_port: self.peer_port,
            sequence: if flags == ACK && self.sent && self.state == State::Open {
                self.next_tx
                    .wrapping_add(self.tx_len as u32 + u32::from(self.write_shutdown && self.tx_len == 0))
            } else {
                self.next_tx
            },
            acknowledgement: self.next_rx,
            flags,
            window: if self.read_shutdown { self.rx.len() } else { self.rx.len() - self.rx_len } as u16,
            urgent_pointer: 0,
            options: &[],
            payload,
        };
        let mut frame = [0; 60 + MSS];
        if let Some(length) = tcp_frame(
            mac,
            self.peer_mac,
            self.local,
            self.peer,
            &segment,
            &mut frame,
        ) {
            if send(&frame[..length]) {
                self.ack_pending = false;
                if self.closing && self.fin_acked && self.peer_fin {
                    self.state = State::Free;
                }
                if flags != ACK {
                    self.sent = true;
                    self.last_send = now;
                }
            }
        }
    }
}

impl<const N: usize> SocketTable<N> {
    pub(super) fn tcp_closing_port_in_use(&self, address: [u8; 4], port: u16) -> bool {
        self.connections.iter().any(|connection| {
            connection.state == State::Open
                && connection.closing
                && connection.local_port == port
                && (address == [0; 4] || connection.local == address)
        })
    }

    pub fn tcp_receive(
        &mut self,
        frame: &[u8],
        mac: [u8; 6],
        address: [u8; 4],
        mask: [u8; 4],
        now: u64,
        initial_sequence: impl FnOnce() -> Option<u32>,
    ) -> bool {
        let Some((peer_mac, peer, segment)) = local_tcp(frame, mac, address) else {
            return false;
        };
        if let Some(connection) = self.connections.iter_mut().find(|c| {
            matches!(c.state, State::Handshake | State::SynSent | State::Open)
                && (c.local, c.peer, c.local_port, c.peer_port)
                    == (address, peer, segment.destination_port, segment.source_port)
        }) {
            if connection.peer_mac == peer_mac {
                connection.receive(segment, now);
                if connection.state == State::Open && connection.listener == 0 && connection.fd != 0
                {
                    self.entries[connection.fd as usize - 1].info.state = SocketState::Connected;
                }
            }
            return true;
        }
        if super::ipv4_next_hop(address, Some(mask), None, peer).is_none() {
            return false;
        }
        let Some((index, listener)) = self
            .entries
            .iter().enumerate()
            .find(|(_, entry)| {
                entry.occupied
                    && entry.info.state == SocketState::Listening
                    && entry.info.local_port == segment.destination_port
                    && (entry.info.local_address == [0; 4] || entry.info.local_address == address)
            })
            .map(|(index, entry)| (index as u32 + 1, entry.info))
        else {
            return false;
        };
        if segment.flags & (SYN | ACK | RST | FIN) != SYN || !segment.payload.is_empty() {
            return true;
        }
        let pending = self
            .connections
            .iter()
            .filter(|c| {
                c.state != State::Free
                    && c.fd == 0
                    && c.listener == index
                    && !c.closing
            })
            .count();
        if pending >= listener.backlog as usize {
            return true;
        }
        if let Some(slot) = self.connections.iter_mut().find(|c| c.state == State::Free) {
            if let Some(sequence) = initial_sequence() {
                *slot = Connection {
                    state: State::Handshake,
                    listener: index,
                    local: address,
                    peer,
                    peer_mac,
                    local_port: segment.destination_port,
                    peer_port: segment.source_port,
                    next_rx: segment.sequence.wrapping_add(1),
                    next_tx: sequence,
                    expires: now + CONNECTION_TIMEOUT,
                    ..Connection::EMPTY
                };
            }
        }
        true
    }

    pub fn tcp_transmit(&mut self, mac: [u8; 6], now: u64, mut send: impl FnMut(&[u8]) -> bool) {
        for connection in &mut self.connections {
            if matches!(
                connection.state,
                State::Handshake | State::SynSent | State::Open
            ) {
                connection.transmit(mac, now, &mut send);
            }
        }
    }

    pub fn tcp_connect_status(&self, owner: u64, fd: u32) -> Result<Option<bool>, NetworkError> {
        let info = self.info(owner, fd).ok_or(NetworkError::BadDescriptor)?;
        if info.protocol != SocketProtocol::Tcp {
            return Err(NetworkError::UnsupportedSocket);
        }
        match info.state {
            SocketState::Created | SocketState::Bound => Ok(None),
            SocketState::Connected => Ok(Some(true)),
            SocketState::Connecting => {
                let endpoint = self.find(owner, fd).ok_or(NetworkError::BadDescriptor)? as u32 + 1;
                let entry = self
                    .connections
                    .iter()
                    .find(|c| c.fd == endpoint)
                    .ok_or(NetworkError::BadDescriptor)?;
                match entry.state {
                    State::Refused => Err(NetworkError::ConnectionRefused),
                    State::TimedOut => Err(NetworkError::ConnectionTimedOut),
                    _ => Ok(Some(false)),
                }
            }
            SocketState::Listening => Err(NetworkError::InvalidState),
        }
    }

    pub fn start_tcp_connect(
        &mut self,
        owner: u64,
        fd: u32,
        local: [u8; 4],
        peer: ([u8; 4], u16, [u8; 6]),
        sequence: u32,
        now: u64,
    ) -> Result<(), NetworkError> {
        match self.tcp_connect_status(owner, fd)? {
            Some(true) => return Err(NetworkError::AlreadyConnected),
            Some(false) => return Err(NetworkError::InProgress),
            None => {}
        }
        if peer.1 == 0 {
            return Err(NetworkError::InvalidPort);
        }
        let entry = self.find(owner, fd).ok_or(NetworkError::BadDescriptor)?;
        let info = self.entries[entry].info;
        if info.local_address != [0; 4] && info.local_address != local {
            return Err(NetworkError::InvalidAddress);
        }
        let index = self
            .connections
            .iter()
            .position(|c| c.state == State::Free)
            .ok_or(NetworkError::PortsExhausted)?;
        if info.state == SocketState::Created {
            self.bind(owner, 0, fd, local, 0)?;
        }
        let local_port = self.entries[entry].info.local_port;
        if self.connections.iter().any(|c| {
            c.state != State::Free
                && c.local == local
                && c.local_port == local_port
                && c.peer == peer.0
                && c.peer_port == peer.1
        }) {
            return Err(NetworkError::AddressInUse);
        }
        self.entries[entry].info.local_address = local;
        self.entries[entry].info.state = SocketState::Connecting;
        self.connections[index] = Connection {
            state: State::SynSent,
            fd: entry as u32 + 1,
            local,
            peer: peer.0,
            peer_port: peer.1,
            peer_mac: peer.2,
            local_port,
            next_tx: sequence,
            expires: now.saturating_add(CONNECTION_TIMEOUT),
            ..Connection::EMPTY
        };
        Ok(())
    }

    pub fn accept_tcp(
        &mut self,
        owner: u64,
        listener: u32,
        flags: u64,
    ) -> Result<(u32, [u8; 4], u16), NetworkError> {
        self.accept_tcp_excluding(owner, listener, flags, |_| false)
    }

    pub fn accept_tcp_excluding(
        &mut self,
        owner: u64,
        listener: u32,
        flags: u64,
        reserved: impl Fn(u32) -> bool,
    ) -> Result<(u32, [u8; 4], u16), NetworkError> {
        if self
            .info(owner, listener)
            .ok_or(NetworkError::BadDescriptor)?
            .state
            != SocketState::Listening
        {
            return Err(NetworkError::InvalidState);
        }
        let listener = self.find(owner, listener).ok_or(NetworkError::BadDescriptor)? as u32 + 1;
        let index = self
            .connections
            .iter()
            .position(|c| {
                c.state == State::Open
                    && c.fd == 0
                    && !c.closing
                    && c.listener == listener
            })
            .ok_or(NetworkError::WouldBlock)?;
        let fd = self.create_excluding(owner, 0, 2, 1 | flags, 6, reserved)?;
        let entry = self.find(owner, fd).ok_or(NetworkError::BadDescriptor)?;
        let connection = &mut self.connections[index];
        connection.fd = entry as u32 + 1;
        let (local, local_port, peer, peer_port) = (
            connection.local,
            connection.local_port,
            connection.peer,
            connection.peer_port,
        );
        self.entries[entry].info.local_address = local;
        self.entries[entry].info.local_port = local_port;
        self.entries[entry].info.state = SocketState::Connected;
        Ok((fd, peer, peer_port))
    }

    pub fn tcp_peer(&self, owner: u64, fd: u32) -> Result<Option<([u8; 4], u16)>, NetworkError> {
        let info = self.info(owner, fd).ok_or(NetworkError::BadDescriptor)?;
        if info.state != SocketState::Connected {
            return Ok(None);
        }
        let endpoint = self.find(owner, fd).ok_or(NetworkError::BadDescriptor)? as u32 + 1;
        Ok(self.connections.iter()
            .find(|connection| connection.state == State::Open
                && connection.fd == endpoint)
            .map(|connection| (connection.peer, connection.peer_port)))
    }

    fn connected(&mut self, owner: u64, fd: u32) -> Result<&mut Connection, NetworkError> {
        let endpoint = self.find(owner, fd).ok_or(NetworkError::BadDescriptor)? as u32 + 1;
        let connection = self
            .connections
            .iter_mut()
            .find(|c| c.state != State::Free && c.fd == endpoint)
            .ok_or(NetworkError::BadDescriptor)?;
        match connection.state {
            State::Open => Ok(connection),
            State::Refused => Err(NetworkError::ConnectionRefused),
            State::TimedOut => Err(NetworkError::ConnectionTimedOut),
            State::Reset => Err(NetworkError::ConnectionReset),
            _ => Err(NetworkError::WouldBlock),
        }
    }

    pub fn tcp_shutdown(&mut self, owner: u64, fd: u32, how: u32, now: u64) -> Result<(), NetworkError> {
        if how > 2 {
            return Err(NetworkError::InvalidState);
        }
        let info = self.info(owner, fd).ok_or(NetworkError::BadDescriptor)?;
        if info.protocol != SocketProtocol::Tcp {
            return Err(NetworkError::UnsupportedSocket);
        }
        if info.state != SocketState::Connected {
            return Err(NetworkError::NotConnected);
        }
        let connection = self.connected(owner, fd)?;
        if how != 1 {
            connection.read_shutdown = true;
            connection.ack_pending |= connection.rx_len != 0;
        }
        if how != 0 && !connection.write_shutdown {
            connection.write_shutdown = true;
            connection.expires = now.saturating_add(CONNECTION_TIMEOUT);
        }
        Ok(())
    }

    pub fn tcp_read(&mut self, owner: u64, fd: u32, out: &mut [u8]) -> Result<usize, NetworkError> {
        let c = self.connected(owner, fd)?;
        let length = out.len().min(c.rx_len);
        if length == 0 {
            return if c.peer_fin || c.read_shutdown || out.is_empty() {
                Ok(0)
            } else {
                Err(NetworkError::WouldBlock)
            };
        }
        out[..length].copy_from_slice(&c.rx[..length]);
        c.rx.copy_within(length..c.rx_len, 0);
        c.rx_len -= length;
        c.ack_pending = true;
        Ok(length)
    }

    pub fn tcp_write(
        &mut self,
        owner: u64,
        fd: u32,
        bytes: &[u8],
        now: u64,
    ) -> Result<usize, NetworkError> {
        let c = self.connected(owner, fd)?;
        let length = bytes.len().min(MSS).min(c.peer_window as usize);
        if bytes.is_empty() {
            return Ok(0);
        }
        if c.write_shutdown {
            return Err(NetworkError::BrokenPipe);
        }
        if length == 0 || c.tx_len != 0 {
            return Err(NetworkError::WouldBlock);
        }
        c.tx[..length].copy_from_slice(&bytes[..length]);
        c.tx_len = length;
        c.expires = now + CONNECTION_TIMEOUT;
        Ok(length)
    }

    pub fn tcp_events(&self, owner: u64, fd: u32) -> Result<(bool, bool, bool), NetworkError> {
        let info = self.info(owner, fd).ok_or(NetworkError::BadDescriptor)?;
        let fd = self.find(owner, fd).ok_or(NetworkError::BadDescriptor)? as u32 + 1;
        let mut readable = false;
        let mut writable = false;
        let mut failed = false;
        for c in &self.connections {
            if c.state == State::Free {
                continue;
            }
            if info.state == SocketState::Listening {
                readable |= c.listener == fd && c.fd == 0 && !c.closing && c.state == State::Open;
            } else if c.fd == fd {
                failed = matches!(c.state, State::Reset | State::Refused | State::TimedOut);
                readable = c.peer_fin || c.read_shutdown || c.rx_len != 0;
                writable = c.state == State::Open && (c.write_shutdown || (c.tx_len == 0 && c.peer_window != 0));
            }
        }
        Ok((readable, writable, failed))
    }

    pub(super) fn release_connections(&mut self, fd: u32) {
        for c in &mut self.connections {
            if c.state == State::Free || c.closing {
                continue;
            }
            if c.fd == 0 {
                if c.listener == fd {
                    c.state = State::Free;
                }
            } else if c.fd == fd {
                if c.state != State::Open || (c.fin_acked && c.peer_fin && !c.ack_pending) {
                    c.state = State::Free;
                } else {
                    c.fd = 0;
                    c.closing = true;
                    c.write_shutdown = true;
                    c.read_shutdown = true;
                    c.rx_len = 0;
                    c.expires = 0;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::{EthernetFrame, Ipv4Packet};

    #[test]
    fn tcp_shutdown_flushes_data_then_fin_and_still_receives() {
        let mut sockets = SocketTable::<2>::new();
        let fd = sockets.create(1, 1000, 2, 1, 6).unwrap();
        let mac = [2, 0, 0, 0, 0, 1];
        let peer = ([192, 0, 2, 20], 22, [2, 0, 0, 0, 0, 2]);
        sockets.start_tcp_connect(1, fd, [192, 0, 2, 10], peer, 100, 0).unwrap();
        sockets.entries[0].info.state = SocketState::Connected;
        sockets.connections[0].state = State::Open;
        sockets.connections[0].peer_window = 1024;
        assert_eq!(sockets.tcp_write(1, fd, b"a", 1), Ok(1));
        sockets.tcp_shutdown(1, fd, 1, 2).unwrap();
        assert_eq!(sockets.tcp_write(1, fd, b"x", 2), Err(NetworkError::BrokenPipe));

        for (tick, flags, sequence, payload) in [
            (2, ACK | 8, 100, &b"a"[..]),
            (3, ACK | FIN, 101, &b""[..]),
        ] {
            let mut sent = false;
            sockets.tcp_transmit(mac, tick, |frame| {
                let ethernet = EthernetFrame::parse(frame).unwrap();
                let ip = Ipv4Packet::parse(ethernet.payload).unwrap();
                let segment = TcpSegment::parse(&ip).unwrap();
                assert_eq!((segment.flags, segment.sequence, segment.payload), (flags, sequence, payload));
                sent = true;
                true
            });
            assert!(sent);
            sockets.connections[0].receive(TcpSegment {
                source_port: 22, destination_port: sockets.entries[0].info.local_port,
                sequence: 0, acknowledgement: sequence + 1,
                flags: if tick == 3 { ACK | FIN } else { ACK }, window: 1024,
                urgent_pointer: 0, options: &[],
                payload: if tick == 3 { b"b" } else { b"" },
            }, tick);
        }
        sockets.tcp_transmit(mac, 4, |_| true);
        let mut byte = [0];
        assert_eq!(sockets.tcp_read(1, fd, &mut byte), Ok(1));
        assert_eq!(&byte, b"b");
        assert_eq!(sockets.tcp_read(1, fd, &mut byte), Ok(0));
        assert!(sockets.connections[0].state == State::Open);
    }

    #[test]
    fn inherited_socket_stays_live_after_parent_close_and_number_reuse() {
        use crate::linux_abi::DescriptorControl;

        let mut sockets = SocketTable::<4>::new();
        let fd = sockets.create(3, 1000, 2, 1, 6).unwrap();
        let peer = ([192, 0, 2, 20], 22, [2, 0, 0, 0, 0, 2]);
        sockets.start_tcp_connect(3, fd, [192, 0, 2, 10], peer, 100, 0).unwrap();
        sockets.connections[0].state = State::Open;
        sockets.connections[0].peer_window = 128;
        sockets.inherit(3, 4).unwrap();
        sockets.fcntl(4, fd, DescriptorControl::SetNonblocking(true)).unwrap();
        assert!(sockets.info(3, fd).unwrap().nonblocking);
        sockets.close(3, fd).unwrap();
        assert_eq!(sockets.create(3, 1000, 2, 1, 6), Ok(fd));
        assert_eq!(sockets.tcp_write(3, fd, b"x", 1), Err(NetworkError::BadDescriptor));
        assert_eq!(sockets.tcp_write(4, fd, b"x", 1), Ok(1));
        sockets.close(4, fd).unwrap();
        assert!(sockets.connections[0].closing);
    }

    #[test]
    fn reconnect_uses_a_different_ephemeral_port_while_fin_is_pending() {
        let mut sockets = SocketTable::<4>::new();
        let local = [192, 0, 2, 10];
        let peer = ([192, 0, 2, 20], 22, [2, 0, 0, 0, 0, 2]);
        let fd = sockets.create(3, 1000, 2, 1, 6).unwrap();
        sockets.start_tcp_connect(3, fd, local, peer, 100, 0).unwrap();
        let first_port = sockets.info(3, fd).unwrap().local_port;
        sockets.connections[0].state = State::Open;
        sockets.close(3, fd).unwrap();

        let fd = sockets.create(3, 1000, 2, 1, 6).unwrap();
        sockets.start_tcp_connect(3, fd, local, peer, 200, 1).unwrap();
        assert_ne!(sockets.info(3, fd).unwrap().local_port, first_port);
    }

    #[test]
    fn partial_ack_retransmits_only_unconfirmed_bytes_across_sequence_wrap() {
        let mut connection = Connection {
            state: State::Open,
            fd: 3,
            local: [192, 0, 2, 10],
            peer: [192, 0, 2, 20],
            local_port: 2222,
            peer_port: 2223,
            next_tx: u32::MAX - 1,
            next_rx: 8,
            tx_len: 5,
            expires: CONNECTION_TIMEOUT,
            ..Connection::EMPTY
        };
        connection.tx[..5].copy_from_slice(b"hello");
        let mac = [2, 0, 0, 0, 0, 1];
        connection.transmit(mac, 0, &mut |_| true);
        let ack = TcpSegment {
            source_port: connection.peer_port,
            destination_port: connection.local_port,
            sequence: 8,
            acknowledgement: 0,
            flags: ACK,
            window: 1024,
            urgent_pointer: 0,
            options: &[],
            payload: &[],
        };
        connection.receive(ack, 1);
        assert_eq!((connection.next_tx, connection.tx_len), (0, 3));
        let mut retransmitted = false;
        connection.transmit(mac, RETRY_TICKS, &mut |frame| {
            let ethernet = EthernetFrame::parse(frame).unwrap();
            let ip = Ipv4Packet::parse(ethernet.payload).unwrap();
            let segment = TcpSegment::parse(&ip).unwrap();
            assert_eq!(segment.sequence, 0);
            assert_eq!(segment.payload, b"llo");
            retransmitted = true;
            true
        });
        assert!(retransmitted);
        connection.receive(TcpSegment { acknowledgement: 3, ..ack }, RETRY_TICKS + 1);
        assert_eq!((connection.next_tx, connection.tx_len), (3, 0));
        assert!(!connection.sent);
    }

    #[test]
    fn outbound_handshake_checks_ack_and_gateway_mac_and_enables_data() {
        let mut sockets = SocketTable::<4>::new();
        let fd = sockets.create(3, 1000, 2, 1, 6).unwrap();
        let local = [192, 0, 2, 10];
        let peer = [198, 51, 100, 20];
        let mac = [2, 0, 0, 0, 0, 1];
        let gateway = [2, 0, 0, 0, 0, 2];
        let mask = [255, 255, 255, 0];
        sockets
            .start_tcp_connect(3, fd, local, (peer, 22, gateway), 100, 0)
            .unwrap();
        let port = sockets.info(3, fd).unwrap().local_port;
        let mut syn_sent = false;
        sockets.tcp_transmit(mac, 0, |frame| {
            let ethernet = EthernetFrame::parse(frame).unwrap();
            assert_eq!(ethernet.destination, gateway);
            let ip = Ipv4Packet::parse(ethernet.payload).unwrap();
            assert_eq!(ip.destination, peer);
            let segment = TcpSegment::parse(&ip).unwrap();
            assert_eq!((segment.flags, segment.sequence), (SYN, 100));
            syn_sent = true;
            true
        });
        assert!(syn_sent);

        for (source, acknowledgement) in [(gateway, 100), ([2, 0, 0, 0, 0, 3], 101), (gateway, 101)]
        {
            let response = TcpSegment {
                source_port: 22,
                destination_port: port,
                sequence: 200,
                acknowledgement,
                flags: SYN | ACK,
                window: 1024,
                urgent_pointer: 0,
                options: &[],
                payload: &[],
            };
            let mut frame = [0; 60];
            let length = tcp_frame(source, mac, peer, local, &response, &mut frame).unwrap();
            assert!(sockets.tcp_receive(&frame[..length], mac, local, mask, 1, || None));
            assert_eq!(
                sockets.tcp_connect_status(3, fd),
                Ok(Some(source == gateway && acknowledgement == 101)),
            );
        }
        let mut ack_sent = false;
        sockets.tcp_transmit(mac, 1, |frame| {
            let ethernet = EthernetFrame::parse(frame).unwrap();
            let ip = Ipv4Packet::parse(ethernet.payload).unwrap();
            let segment = TcpSegment::parse(&ip).unwrap();
            assert_eq!(
                (segment.flags, segment.sequence, segment.acknowledgement),
                (ACK, 101, 201)
            );
            ack_sent = true;
            true
        });
        assert!(ack_sent);
        assert_eq!(sockets.tcp_write(3, fd, b"hello", 2), Ok(5));
    }

    #[test]
    fn outbound_timeout_reports_poll_error_and_close_releases_slot() {
        let mut sockets = SocketTable::<1>::new();
        let fd = sockets.create(3, 1000, 2, 1, 6).unwrap();
        let local = [192, 0, 2, 10];
        let peer = ([192, 0, 2, 20], 22, [2, 0, 0, 0, 0, 2]);
        sockets
            .start_tcp_connect(3, fd, local, peer, 100, 0)
            .unwrap();
        sockets.tcp_transmit([2, 0, 0, 0, 0, 1], CONNECTION_TIMEOUT, |_| panic!());
        assert_eq!(
            sockets.tcp_connect_status(3, fd),
            Err(NetworkError::ConnectionTimedOut)
        );
        assert_eq!(sockets.tcp_events(3, fd), Ok((false, false, true)));
        sockets.close(3, fd).unwrap();
        let fd = sockets.create(3, 1000, 2, 1, 6).unwrap();
        sockets
            .start_tcp_connect(3, fd, local, peer, 200, CONNECTION_TIMEOUT)
            .unwrap();
    }
}

use super::{local_tcp, tcp_frame, NetworkError, SocketState, SocketTable, TcpSegment};

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
    Open,
    Reset,
}

#[derive(Clone, Copy)]
pub(super) struct Connection {
    state: State,
    owner: u64,
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
    ack_pending: bool,
    fin_acked: bool,
    sent: bool,
    last_send: u64,
    expires: u64,
}

impl Connection {
    pub(super) const EMPTY: Self = Self {
        state: State::Free,
        owner: 0,
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
        if segment.flags & ACK == 0 || segment.flags & SYN != 0 {
            return;
        }
        if segment.sequence != self.next_rx {
            self.ack_pending = true;
            return;
        }
        let in_flight = if self.sent {
            self.tx_len as u32 + u32::from(self.closing && self.tx_len == 0)
        } else {
            0
        };
        if segment.acknowledgement == self.next_tx.wrapping_add(in_flight) {
            self.peer_window = segment.window;
            if in_flight != 0 {
                self.next_tx = segment.acknowledgement;
                if self.closing && self.tx_len == 0 {
                    self.fin_acked = true;
                }
                self.tx_len = 0;
                self.sent = false;
            }
        } else if segment.acknowledgement != self.next_tx {
            self.ack_pending = true;
            return;
        }
        if segment.payload.is_empty() && segment.flags & FIN == 0 {
            if self.fin_acked && self.peer_fin && !self.ack_pending {
                self.state = State::Free;
            }
            return;
        }
        if !self.peer_fin && segment.payload.len() <= self.rx.len() - self.rx_len {
            let end = self.rx_len + segment.payload.len();
            self.rx[self.rx_len..end].copy_from_slice(segment.payload);
            self.rx_len = end;
            self.peer_fin = segment.flags & FIN != 0;
            self.next_rx = self
                .next_rx
                .wrapping_add(segment.payload.len() as u32 + u32::from(self.peer_fin));
        }
        self.ack_pending = true;
    }

    fn transmit(&mut self, mac: [u8; 6], now: u64, send: &mut impl FnMut(&[u8]) -> bool) {
        if self.closing && self.expires == 0 {
            self.expires = now + CONNECTION_TIMEOUT;
        }
        let timed = self.state == State::Handshake || self.fd == 0 || self.tx_len != 0;
        if timed && now >= self.expires {
            self.fail();
            return;
        }
        let retry = !self.sent || now.saturating_sub(self.last_send) >= RETRY_TICKS;
        let (flags, payload) = if self.state == State::Handshake && retry {
            (SYN | ACK, &[][..])
        } else if self.tx_len != 0 && retry {
            (ACK | 8, &self.tx[..self.tx_len])
        } else if self.closing && !self.fin_acked && self.tx_len == 0 && retry {
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
                    .wrapping_add(self.tx_len as u32 + u32::from(self.closing && self.tx_len == 0))
            } else {
                self.next_tx
            },
            acknowledgement: self.next_rx,
            flags,
            window: (self.rx.len() - self.rx_len) as u16,
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
                if self.fin_acked && self.peer_fin {
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
    pub fn tcp_receive(
        &mut self,
        frame: &[u8],
        mac: [u8; 6],
        address: [u8; 4],
        mask: [u8; 4],
        now: u64,
        initial_sequence: impl FnOnce() -> Option<u32>,
    ) -> bool {
        let Some((peer_mac, peer, segment)) = local_tcp(frame, mac, address, mask) else {
            return false;
        };
        if let Some(connection) = self.connections.iter_mut().find(|c| {
            matches!(c.state, State::Handshake | State::Open)
                && (c.local, c.peer, c.local_port, c.peer_port)
                    == (address, peer, segment.destination_port, segment.source_port)
        }) {
            if connection.peer_mac == peer_mac {
                connection.receive(segment, now);
            }
            return true;
        }
        let Some(listener) = self
            .entries
            .iter()
            .find(|entry| {
                entry.occupied
                    && entry.info.state == SocketState::Listening
                    && entry.info.local_port == segment.destination_port
                    && (entry.info.local_address == [0; 4] || entry.info.local_address == address)
            })
            .map(|entry| entry.info)
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
                    && c.listener == listener.descriptor
                    && c.owner == listener.process_id
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
                    owner: listener.process_id,
                    listener: listener.descriptor,
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
            if matches!(connection.state, State::Handshake | State::Open) {
                connection.transmit(mac, now, &mut send);
            }
        }
    }

    pub fn accept_tcp(
        &mut self,
        owner: u64,
        listener: u32,
        flags: u64,
    ) -> Result<(u32, [u8; 4], u16), NetworkError> {
        if self
            .info(owner, listener)
            .ok_or(NetworkError::BadDescriptor)?
            .state
            != SocketState::Listening
        {
            return Err(NetworkError::InvalidState);
        }
        let index = self
            .connections
            .iter()
            .position(|c| {
                c.state == State::Open
                    && c.fd == 0
                    && !c.closing
                    && c.owner == owner
                    && c.listener == listener
            })
            .ok_or(NetworkError::WouldBlock)?;
        let fd = self.create(owner, 0, 2, 1 | flags, 6)?;
        let connection = &mut self.connections[index];
        connection.fd = fd;
        let (local, local_port, peer, peer_port) = (
            connection.local,
            connection.local_port,
            connection.peer,
            connection.peer_port,
        );
        let entry = self.find(owner, fd).ok_or(NetworkError::BadDescriptor)?;
        self.entries[entry].info.local_address = local;
        self.entries[entry].info.local_port = local_port;
        self.entries[entry].info.state = SocketState::Connected;
        Ok((fd, peer, peer_port))
    }

    fn connected(&mut self, owner: u64, fd: u32) -> Result<&mut Connection, NetworkError> {
        let connection = self
            .connections
            .iter_mut()
            .find(|c| c.state != State::Free && c.fd == fd && c.owner == owner)
            .ok_or(NetworkError::BadDescriptor)?;
        if connection.state == State::Reset {
            return Err(NetworkError::ConnectionReset);
        }
        Ok(connection)
    }

    pub fn tcp_read(&mut self, owner: u64, fd: u32, out: &mut [u8]) -> Result<usize, NetworkError> {
        let c = self.connected(owner, fd)?;
        let length = out.len().min(c.rx_len);
        if length == 0 {
            return if c.peer_fin || out.is_empty() {
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
        let mut readable = false;
        let mut writable = false;
        let mut failed = false;
        for c in &self.connections {
            if c.owner != owner || c.state == State::Free {
                continue;
            }
            if info.state == SocketState::Listening {
                readable |= c.listener == fd && c.fd == 0 && !c.closing && c.state == State::Open;
            } else if c.fd == fd {
                failed = c.state == State::Reset;
                readable = c.peer_fin || c.rx_len != 0;
                writable = !failed && c.tx_len == 0 && c.peer_window != 0;
            }
        }
        Ok((readable, writable, failed))
    }

    pub(super) fn release_connections(&mut self, owner: u64, fd: Option<u32>) {
        for c in &mut self.connections {
            if c.owner != owner || c.state == State::Free || c.closing {
                continue;
            }
            if c.fd == 0 {
                if fd.is_none_or(|fd| c.listener == fd) {
                    c.state = State::Free;
                }
            } else if fd.is_none_or(|fd| c.fd == fd) {
                if c.state == State::Reset {
                    c.state = State::Free;
                } else {
                    c.fd = 0;
                    c.closing = true;
                    c.expires = 0;
                }
            }
        }
    }
}

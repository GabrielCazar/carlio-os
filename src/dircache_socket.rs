//! Local socket transport for the directory broker.
use crate::{dircache::*, linux_abi::*};

pub const FIRST_FD: u64 = 192;
const SLOTS: usize = 16;
// A canonical v1 response with 34 decimal IDs and a 255-byte name fits here.
pub const PACKET_SIZE: usize = 1024;
const _: () = assert!(PACKET_SIZE >= 32 + MAX_NAME + 34 * 11);

pub fn is_descriptor(fd: u64) -> bool {
    (FIRST_FD..FIRST_FD + SLOTS as u64).contains(&fd)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    New,
    Bound,
    Listening(u16),
    Waiting,
    Connected,
}

struct Socket {
    owner: Peer,
    state: State,
    peer: Option<usize>,
    cloexec: bool,
    server: bool,
    packet: [u8; PACKET_SIZE],
    len: usize,
    ticket: Option<Ticket>,
    error: Option<u16>,
}

#[derive(Clone, Copy)]
pub struct Audit {
    pub sequence: u64,
    pub pid: u64,
    pub operation: &'static str,
    pub errno: u16,
}

impl Audit {
    /// Fixed binary record stored alongside existing kernel log records.
    pub const SIZE: usize = 36;

    pub fn encode(self) -> [u8; Self::SIZE] {
        let mut record = [0; 36];
        record[..8].copy_from_slice(b"DIRAUD01");
        record[8..16].copy_from_slice(&self.sequence.to_le_bytes());
        record[16..24].copy_from_slice(&self.pid.to_le_bytes());
        record[24..26].copy_from_slice(&self.errno.to_le_bytes());
        let operation = self.operation.as_bytes();
        let len = operation.len().min(10);
        record[26..26 + len].copy_from_slice(&operation[..len]);
        record
    }
}

pub struct Endpoint {
    enabled: bool,
    broker: Broker<SLOTS>,
    backend: Option<usize>,
    sockets: [Option<Socket>; SLOTS],
    sequence: u64,
    audit: [Option<Audit>; 64],
    next_event: usize,
    audit_sequence: u64,
}

impl Endpoint {
    pub const fn new(enabled: bool, timeout: u64) -> Self {
        Self {
            enabled,
            broker: Broker::new(enabled, timeout),
            backend: None,
            sockets: [const { None }; SLOTS],
            sequence: 0,
            audit: [None; 64],
            next_event: 0,
            audit_sequence: 0,
        }
    }

    pub fn audit(&self) -> [Option<Audit>; 64] {
        let mut events = self.audit;
        events.rotate_left(self.next_event);
        events
    }

    /// Absolute byte position of the first retained record in this boot.
    pub fn audit_start(&self) -> u64 {
        self.audit_sequence.saturating_sub(self.audit.len() as u64) * Audit::SIZE as u64
    }

    pub fn read_audit(&self, mut offset: u64, output: &mut [u8]) -> Result<usize, u16> {
        let size = Audit::SIZE as u64;
        if offset < self.audit_start() {
            return Err(EOVERFLOW);
        }
        let mut amount = 0;
        while amount < output.len() && offset / size < self.audit_sequence {
            let slot = (offset / size % self.audit.len() as u64) as usize;
            let record = self.audit[slot].ok_or(EIO)?.encode();
            let index = (offset % size) as usize;
            let count = (record.len() - index).min(output.len() - amount);
            output[amount..amount + count].copy_from_slice(&record[index..index + count]);
            amount += count;
            offset += count as u64;
        }
        Ok(amount)
    }

    pub fn record(&mut self, pid: u64, operation: &'static str, errno: u16) {
        let Some(sequence) = self
            .audit_sequence
            .checked_add(1)
            .filter(|sequence| *sequence <= u64::MAX / Audit::SIZE as u64)
        else {
            return;
        };
        self.audit_sequence = sequence;
        self.audit[self.next_event] = Some(Audit {
            sequence,
            pid,
            operation,
            errno,
        });
        self.next_event = (self.next_event + 1) % self.audit.len();
    }

    fn slot(&self, pid: u64, fd: u64) -> Result<usize, u16> {
        if !is_descriptor(fd) {
            return Err(EBADF);
        }
        let index = (fd - FIRST_FD) as usize;
        self.sockets[index]
            .as_ref()
            .filter(|s| s.owner.pid == pid)
            .ok_or(EBADF)?;
        Ok(index)
    }

    fn allocate(&mut self, pid: u64, flags: u64) -> Result<usize, u16> {
        let index = self
            .sockets
            .iter()
            .position(Option::is_none)
            .ok_or(ENFILE)?;
        self.sequence = self.sequence.checked_add(1).ok_or(EOVERFLOW)?;
        self.sockets[index] = Some(Socket {
            owner: Peer {
                pid,
                generation: self.sequence,
            },
            state: State::New,
            peer: None,
            cloexec: flags & O_CLOEXEC != 0,
            server: false,
            packet: [0; PACKET_SIZE],
            len: 0,
            ticket: None,
            error: None,
        });
        Ok(index)
    }

    pub fn socket(&mut self, pid: u64, kind: u64, protocol: u64) -> Result<u64, u16> {
        if kind & !(O_CLOEXEC | 0x800) != 5 || protocol != 0 {
            return Err(EOPNOTSUPP);
        }
        if !self.enabled {
            return Err(EACCES);
        }
        self.allocate(pid, kind)
            .map(|index| FIRST_FD + index as u64)
    }

    pub fn address(
        &mut self,
        pid: u64,
        fd: u64,
        path: &[u8],
        bind: bool,
        authorized: bool,
    ) -> Result<(), u16> {
        let index = self.slot(pid, fd)?;
        if path != SOCKET_PATH.as_bytes() {
            return Err(ENOENT);
        }
        let socket = self.sockets[index].as_ref().unwrap();
        if socket.state != State::New {
            return Err(EINVAL);
        }
        if bind {
            if !authorized {
                return Err(EACCES);
            }
            self.broker
                .register(socket.owner, authorized)
                .map_err(broker_errno)?;
            self.backend = Some(index);
            self.sockets[index].as_mut().unwrap().state = State::Bound;
        } else {
            let listener = self
                .backend
                .and_then(|i| self.sockets[i].as_ref())
                .ok_or(ECONNREFUSED)?;
            let State::Listening(backlog) = listener.state else {
                return Err(ECONNREFUSED);
            };
            let waiting = self
                .sockets
                .iter()
                .flatten()
                .filter(|s| s.state == State::Waiting)
                .count();
            if waiting >= usize::from(backlog) {
                return Err(EAGAIN);
            }
            self.sockets[index].as_mut().unwrap().state = State::Waiting;
        }
        Ok(())
    }

    pub fn listen(&mut self, pid: u64, fd: u64, backlog: u16) -> Result<(), u16> {
        let index = self.slot(pid, fd)?;
        if Some(index) != self.backend {
            return Err(EINVAL);
        }
        self.sockets[index].as_mut().unwrap().state = State::Listening(backlog);
        Ok(())
    }

    pub fn accept(&mut self, pid: u64, fd: u64, flags: u64) -> Result<u64, u16> {
        let index = self.slot(pid, fd)?;
        if !matches!(
            self.sockets[index].as_ref().unwrap().state,
            State::Listening(_)
        ) {
            return Err(EINVAL);
        }
        let client = self
            .sockets
            .iter()
            .enumerate()
            .filter_map(|(i, socket)| socket.as_ref().map(|socket| (i, socket)))
            .filter(|(_, socket)| socket.state == State::Waiting)
            .min_by_key(|(_, socket)| socket.owner.generation)
            .map(|(i, _)| i)
            .ok_or(EAGAIN)?;
        let server = self.allocate(pid, flags)?;
        for (index, peer) in [(client, server), (server, client)] {
            let socket = self.sockets[index].as_mut().unwrap();
            socket.state = State::Connected;
            socket.peer = Some(peer);
            socket.server = index == server;
        }
        Ok(FIRST_FD + server as u64)
    }

    fn expire(&mut self, now: u64) {
        let mut expired = [None; SLOTS];
        let mut count = 0;
        self.broker.expire(now, |peer, ticket, _| {
            expired[count] = Some((peer, ticket));
            count += 1;
        });
        for (peer, ticket) in expired.into_iter().flatten() {
            if let Some(client) = self.sockets.iter_mut().flatten().find(|s| s.owner == peer) {
                client.ticket = None;
                client.error = Some(ETIMEDOUT);
                if let Some(server) = client.peer.and_then(|i| self.sockets[i].as_mut()) {
                    if server.ticket == Some(ticket) {
                        server.ticket = None;
                        server.len = 0;
                    }
                }
            }
            self.record(peer.pid, "timeout", ETIMEDOUT);
        }
    }

    pub fn send(&mut self, pid: u64, fd: u64, packet: &[u8], now: u64) -> Result<usize, u16> {
        self.expire(now);
        let index = self.slot(pid, fd)?;
        let source = self.sockets[index].as_ref().unwrap();
        if source.state == State::Waiting {
            return Err(EAGAIN);
        }
        let target_index = source.peer.ok_or(ENOTCONN)?;
        let target = self.sockets[target_index].as_ref().unwrap();
        if target.len != 0 || source.len != 0 || target.error.is_some() || source.error.is_some() {
            return Err(EAGAIN);
        }
        let backend = self
            .backend
            .and_then(|i| self.sockets[i].as_ref())
            .ok_or(ECONNRESET)?;
        let mut encoded = [0; PACKET_SIZE];
        let (bytes, ticket) = if source.server {
            let reply = Reply::decode(packet).map_err(broker_errno)?;
            if source.ticket != Some(reply.ticket) || target.ticket != source.ticket {
                return Err(EINVAL);
            }
            let recipient = self
                .broker
                .respond(backend.owner, &reply, now)
                .map_err(broker_errno)?;
            debug_assert_eq!(recipient, target.owner);
            (packet, None)
        } else {
            if source.ticket.is_some() || target.ticket.is_some() {
                return Err(EAGAIN);
            }
            let lookup = Lookup::decode(packet).map_err(broker_errno)?;
            let request = self
                .broker
                .lookup(source.owner, lookup.name, now)
                .map_err(broker_errno)?;
            let bytes = request.encode(&mut encoded).map_err(broker_errno)?;
            (bytes, Some(request.ticket))
        };
        self.sockets[index].as_mut().unwrap().ticket = ticket;
        let target = self.sockets[target_index].as_mut().unwrap();
        target.ticket = ticket;
        target.packet[..bytes.len()].copy_from_slice(bytes);
        target.len = bytes.len();
        self.record(pid, if ticket.is_some() { "lookup" } else { "reply" }, 0);
        Ok(packet.len())
    }

    pub fn receive(
        &mut self,
        pid: u64,
        fd: u64,
        buffer: &mut [u8],
        now: u64,
    ) -> Result<usize, u16> {
        self.expire(now);
        let index = self.slot(pid, fd)?;
        let socket = self.sockets[index].as_mut().unwrap();
        if let Some(error) = socket.error.take() {
            return Err(error);
        }
        if socket.len == 0 {
            return match socket.state {
                State::Connected if socket.peer.is_none() => Ok(0),
                State::Connected | State::Waiting => Err(EAGAIN),
                _ => Err(ENOTCONN),
            };
        }
        let len = socket.len.min(buffer.len());
        buffer[..len].copy_from_slice(&socket.packet[..len]);
        socket.len = 0;
        Ok(len)
    }

    pub fn name(&self, pid: u64, fd: u64) -> Result<bool, u16> {
        let socket = self.sockets[self.slot(pid, fd)?].as_ref().unwrap();
        Ok(socket.server || matches!(socket.state, State::Bound | State::Listening(_)))
    }

    pub fn poll(&mut self, pid: u64, fd: u64, events: u16, now: u64) -> Result<u16, u16> {
        self.expire(now);
        let socket = self.sockets[self.slot(pid, fd)?].as_ref().unwrap();
        let mut ready = 0;
        if socket.len != 0
            || (matches!(socket.state, State::Listening(_))
                && self
                    .sockets
                    .iter()
                    .flatten()
                    .any(|s| s.state == State::Waiting))
        {
            ready |= POLLIN;
        }
        if socket
            .peer
            .and_then(|i| self.sockets[i].as_ref())
            .is_some_and(|peer| {
                peer.len == 0
                    && socket.len == 0
                    && peer.error.is_none()
                    && socket.error.is_none()
                    && (socket.ticket.is_none() || socket.server)
            })
        {
            ready |= POLLOUT;
        }
        Ok((ready & events)
            | if socket.error.is_some() { POLLERR } else { 0 }
            | if socket.state == State::Connected && socket.peer.is_none() {
                0x10
            } else {
                0
            })
    }

    pub fn close(&mut self, pid: u64, fd: u64) -> Result<(), u16> {
        let index = self.slot(pid, fd)?;
        let socket = self.sockets[index].take().unwrap();
        self.broker.disconnect(socket.owner, |_, _, _| {});
        if Some(index) == self.backend {
            self.backend = None;
            for socket in self.sockets.iter_mut().flatten() {
                if matches!(socket.state, State::Connected | State::Waiting) {
                    socket.state = State::Connected;
                    socket.peer = None;
                    socket.len = 0;
                    socket.ticket = None;
                    socket.error = Some(ECONNRESET);
                }
            }
        } else if let Some(peer) = socket.peer.and_then(|i| self.sockets[i].as_mut()) {
            if socket.server {
                self.broker.disconnect(peer.owner, |_, _, _| {});
            }
            peer.peer = None;
            peer.len = 0;
            peer.ticket = None;
            peer.error = Some(ECONNRESET);
        }
        self.record(
            pid,
            if matches!(socket.state, State::Bound | State::Listening(_)) {
                "remove"
            } else {
                "close"
            },
            0,
        );
        Ok(())
    }

    pub fn close_process(&mut self, pid: u64, exec: bool) {
        for index in 0..SLOTS {
            if self.sockets[index].as_ref().is_some_and(|s| {
                s.owner.pid == pid
                    && (!exec || s.cloexec || s.server || Some(index) == self.backend)
            }) {
                let _ = self.close(pid, FIRST_FD + index as u64);
            }
        }
    }
}

fn broker_errno(error: Error) -> u16 {
    match error {
        Error::Disabled | Error::Denied => EACCES,
        Error::Unavailable => ECONNRESET,
        Error::Busy => EBUSY,
        Error::Timeout => ETIMEDOUT,
        Error::Malformed | Error::Stale => EINVAL,
        Error::Overflow => EOVERFLOW,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_reader_handles_partial_reads_and_detects_overrun() {
        let mut endpoint = Endpoint::new(false, 100);
        for pid in 1..=65 {
            endpoint.record(pid, "register", EACCES);
        }
        assert_eq!(endpoint.audit_start(), Audit::SIZE as u64);
        let mut header = [0; 8];
        assert_eq!(endpoint.read_audit(0, &mut header), Err(EOVERFLOW));
        let start = endpoint.audit_start();
        assert_eq!(endpoint.read_audit(start, &mut header[..3]), Ok(3));
        assert_eq!(endpoint.read_audit(start + 3, &mut header[3..]), Ok(5));
        assert_eq!(&header, b"DIRAUD01");
        assert_eq!(
            endpoint.read_audit(65 * Audit::SIZE as u64, &mut header),
            Ok(0)
        );
    }

    #[test]
    fn restricts_registration_and_descriptor_ownership() {
        let mut disabled = Endpoint::new(false, 10);
        assert_eq!(disabled.socket(1, 5, 0), Err(EACCES));

        let mut endpoint = Endpoint::new(true, 10);
        let fd = endpoint.socket(1, 5, 0).unwrap();
        assert_eq!(
            endpoint.address(2, fd, SOCKET_PATH.as_bytes(), true, true),
            Err(EBADF)
        );
        assert_eq!(
            endpoint.address(1, fd, SOCKET_PATH.as_bytes(), true, false),
            Err(EACCES)
        );
        assert_eq!(
            endpoint.address(1, fd, b"/tmp/dircache.sock", true, true),
            Err(ENOENT)
        );
        assert_eq!(
            endpoint.address(1, fd, SOCKET_PATH.as_bytes(), true, true),
            Ok(())
        );
    }

    #[test]
    fn routes_atomic_records_expires_and_revokes_backend() {
        let mut endpoint = Endpoint::new(true, 10);
        let listener = endpoint.socket(1, 5, 0).unwrap();
        endpoint
            .address(1, listener, SOCKET_PATH.as_bytes(), true, true)
            .unwrap();
        endpoint.listen(1, listener, 4).unwrap();
        let client = endpoint.socket(2, 5, 0).unwrap();
        endpoint
            .address(2, client, SOCKET_PATH.as_bytes(), false, false)
            .unwrap();
        assert_eq!(endpoint.poll(1, listener, POLLIN, 0), Ok(POLLIN));
        let server = endpoint.accept(1, listener, 0).unwrap();

        let mut input = [0; PACKET_SIZE];
        let name = "a".repeat(MAX_NAME);
        let query = Lookup {
            ticket: Ticket {
                id: 731,
                generation: 825,
            },
            name: &name,
        }
        .encode(&mut input)
        .unwrap();
        assert!(query.len() > 256);
        assert_eq!(endpoint.send(2, client, query, 0), Ok(query.len()));
        let mut output = [0; PACKET_SIZE];
        let amount = endpoint.receive(1, server, &mut output, 0).unwrap();
        let lookup = Lookup::decode(&output[..amount]).unwrap();
        assert_eq!(lookup.name, name);
        assert_ne!(lookup.ticket.id, 731);
        let ticket = lookup.ticket;

        let mut reply = [0; PACKET_SIZE];
        let response = Reply {
            ticket,
            status: Status::NotFound,
            ttl_ms: 0,
            payload: "",
        }
        .encode(&mut reply)
        .unwrap();
        assert_eq!(endpoint.send(2, client, response, 0), Err(EAGAIN));
        assert_eq!(endpoint.send(1, server, response, 0), Ok(response.len()));
        assert_eq!(endpoint.poll(2, client, POLLIN | POLLOUT, 0), Ok(POLLIN));
        assert_eq!(
            endpoint.receive(2, client, &mut output, 0),
            Ok(response.len())
        );
        assert_eq!(&output[..response.len()], response);
        assert_eq!(endpoint.send(1, server, response, 0), Err(EINVAL));

        endpoint.send(2, client, query, 0).unwrap();
        assert_eq!(endpoint.poll(2, client, POLLIN, 10), Ok(POLLERR));
        assert_eq!(endpoint.receive(2, client, &mut output, 10), Err(ETIMEDOUT));
        assert_eq!(endpoint.receive(1, server, &mut output, 10), Err(EAGAIN));
        endpoint.send(2, client, query, 11).unwrap();
        endpoint.close_process(1, true);
        assert_eq!(
            endpoint.receive(2, client, &mut output, 11),
            Err(ECONNRESET)
        );
        assert_eq!(endpoint.receive(2, client, &mut output, 11), Ok(0));
        assert!(endpoint
            .audit()
            .into_iter()
            .flatten()
            .any(|event| event.operation == "remove"));
    }
}

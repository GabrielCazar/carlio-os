//! Bounded state and wire framing for the opt-in directory lookup channel.
//! The IPC adapter must supply process identities from the kernel and enforce
//! the registration policy before passing its decision to the broker.

pub const SOCKET_PATH: &str = "/run/carlio/dircache.sock";
pub const MAX_FRAME: usize = 64 * 1024;
pub const MAX_NAME: usize = 255;
pub const MAX_TTL_MS: u32 = 60_000;
const HEADER: usize = 32;
const VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Disabled,
    Denied,
    Unavailable,
    Busy,
    Stale,
    Timeout,
    Malformed,
    Overflow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Peer {
    pub pid: u64,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ticket {
    pub id: u64,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Lookup<'a> {
    pub ticket: Ticket,
    pub name: &'a str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Status {
    Found = 0,
    NotFound = 1,
    Unavailable = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Reply<'a> {
    pub ticket: Ticket,
    pub status: Status,
    pub ttl_ms: u32,
    pub payload: &'a str,
}

fn frame(frame: &[u8], kind: u16) -> Result<(Ticket, u32, u32, &str), Error> {
    if !(HEADER..=MAX_FRAME).contains(&frame.len())
        || u16::from_le_bytes([frame[0], frame[1]]) != VERSION
        || u16::from_le_bytes([frame[2], frame[3]]) != kind
        || word(&frame[4..8]) as usize != frame.len()
    {
        return Err(Error::Malformed);
    }
    let ticket = Ticket {
        id: u64::from_le_bytes(frame[8..16].try_into().unwrap()),
        generation: u64::from_le_bytes(frame[16..24].try_into().unwrap()),
    };
    if ticket.id == 0 || ticket.generation == 0 {
        return Err(Error::Malformed);
    }
    let payload = core::str::from_utf8(&frame[HEADER..]).map_err(|_| Error::Malformed)?;
    Ok((ticket, word(&frame[24..28]), word(&frame[28..32]), payload))
}

fn word(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes.try_into().unwrap())
}

fn encode<'a>(
    buffer: &'a mut [u8],
    kind: u16,
    ticket: Ticket,
    first: u32,
    second: u32,
    payload: &str,
) -> Result<&'a [u8], Error> {
    if ticket.id == 0 || ticket.generation == 0 || payload.len() > MAX_FRAME - HEADER {
        return Err(Error::Malformed);
    }
    let length = HEADER + payload.len();
    let frame = buffer.get_mut(..length).ok_or(Error::Overflow)?;
    frame[..2].copy_from_slice(&VERSION.to_le_bytes());
    frame[2..4].copy_from_slice(&kind.to_le_bytes());
    frame[4..8].copy_from_slice(&(length as u32).to_le_bytes());
    frame[8..16].copy_from_slice(&ticket.id.to_le_bytes());
    frame[16..24].copy_from_slice(&ticket.generation.to_le_bytes());
    frame[24..28].copy_from_slice(&first.to_le_bytes());
    frame[28..32].copy_from_slice(&second.to_le_bytes());
    frame[HEADER..].copy_from_slice(payload.as_bytes());
    Ok(frame)
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && !name.contains('/')
        && !name.chars().any(char::is_control)
}

impl Lookup<'_> {
    pub fn encode<'a>(&self, buffer: &'a mut [u8]) -> Result<&'a [u8], Error> {
        if !valid_name(self.name) {
            return Err(Error::Malformed);
        }
        encode(buffer, 1, self.ticket, self.name.len() as u32, 0, self.name)
    }

    pub fn decode(bytes: &[u8]) -> Result<Lookup<'_>, Error> {
        let (ticket, length, flags, name) = frame(bytes, 1)?;
        if flags != 0 || length as usize != name.len() || !valid_name(name) {
            return Err(Error::Malformed);
        }
        Ok(Lookup { ticket, name })
    }
}

// Found payload: name<TAB>uid<TAB>gid, optionally followed by group IDs.
fn valid_record(payload: &str) -> bool {
    fn valid_id(field: &str) -> bool {
        !field.is_empty()
            && field.bytes().all(|byte| byte.is_ascii_digit())
            && field.parse::<u32>().is_ok_and(|id| id != u32::MAX)
    }

    let mut fields = payload.split('\t');
    let Some(name) = fields.next() else {
        return false;
    };
    valid_name(name)
        && fields.next().is_some_and(valid_id)
        && fields.next().is_some_and(valid_id)
        && fields
            .take(33)
            .enumerate()
            .all(|(index, field)| index < 32 && valid_id(field))
}

impl Reply<'_> {
    fn validate(&self) -> Result<(), Error> {
        let valid_payload = match self.status {
            Status::Found => valid_record(self.payload),
            Status::NotFound => self.payload.is_empty(),
            Status::Unavailable => self.payload.is_empty() && self.ttl_ms == 0,
        };
        if !valid_payload || self.ttl_ms > MAX_TTL_MS {
            return Err(Error::Malformed);
        }
        Ok(())
    }

    pub fn encode<'a>(&self, buffer: &'a mut [u8]) -> Result<&'a [u8], Error> {
        self.validate()?;
        encode(
            buffer,
            2,
            self.ticket,
            self.status as u32,
            self.ttl_ms,
            self.payload,
        )
    }

    pub fn decode(bytes: &[u8]) -> Result<Reply<'_>, Error> {
        let (ticket, status, ttl_ms, payload) = frame(bytes, 2)?;
        let status = match status {
            0 => Status::Found,
            1 => Status::NotFound,
            2 => Status::Unavailable,
            _ => return Err(Error::Malformed),
        };
        let reply = Reply {
            ticket,
            status,
            ttl_ms,
            payload,
        };
        reply.validate()?;
        Ok(reply)
    }
}

#[derive(Clone, Copy)]
struct Pending {
    ticket: Ticket,
    client: Peer,
    deadline: u64,
    name: [u8; MAX_NAME],
    name_len: usize,
}

/// Tracks only request ownership and expiry. Directory records stay in userland.
pub struct Broker<const N: usize> {
    enabled: bool,
    timeout: u64,
    generation: u64,
    last_id: u64,
    backend: Option<Peer>,
    pending: [Option<Pending>; N],
}

impl<const N: usize> Broker<N> {
    /// Times are in monotonic ticks from one clock. Zero timeout disables use.
    pub const fn new(enabled: bool, timeout: u64) -> Self {
        Self {
            enabled: enabled && timeout != 0,
            timeout,
            generation: 0,
            last_id: 0,
            backend: None,
            pending: [None; N],
        }
    }

    /// The kernel adapter supplies the combined capability, LSM and boot-policy verdict.
    pub fn register(&mut self, peer: Peer, authorized: bool) -> Result<u64, Error> {
        if !self.enabled {
            return Err(Error::Disabled);
        }
        if !authorized || peer.pid == 0 || peer.generation == 0 {
            return Err(Error::Denied);
        }
        if self.backend.is_some() {
            return Err(Error::Busy);
        }
        let generation = self.generation.checked_add(1).ok_or(Error::Overflow)?;
        self.generation = generation;
        self.backend = Some(peer);
        Ok(generation)
    }

    /// Cancels the peer's requests and invalidates all tickets if it owns the backend.
    pub fn disconnect(&mut self, peer: Peer, mut cancel: impl FnMut(Peer, Ticket, Error)) {
        let backend_lost = self.backend == Some(peer);
        if backend_lost {
            self.backend = None;
        }
        for slot in &mut self.pending {
            if let Some(request) = slot.take_if(|request| backend_lost || request.client == peer) {
                cancel(request.client, request.ticket, Error::Unavailable);
            }
        }
    }

    pub fn expire(&mut self, now: u64, mut cancel: impl FnMut(Peer, Ticket, Error)) {
        for slot in &mut self.pending {
            if let Some(request) = slot.take_if(|request| now >= request.deadline) {
                cancel(request.client, request.ticket, Error::Timeout);
            }
        }
    }

    pub fn lookup<'a>(
        &mut self,
        client: Peer,
        name: &'a str,
        now: u64,
    ) -> Result<Lookup<'a>, Error> {
        if !self.enabled {
            return Err(Error::Disabled);
        }
        if client.pid == 0 || client.generation == 0 {
            return Err(Error::Denied);
        }
        if self.backend.is_none() {
            return Err(Error::Unavailable);
        }
        if !valid_name(name) {
            return Err(Error::Malformed);
        }
        let slot = self
            .pending
            .iter_mut()
            .find(|slot| slot.is_none())
            .ok_or(Error::Busy)?;
        let deadline = now.checked_add(self.timeout).ok_or(Error::Overflow)?;
        let id = self.last_id.checked_add(1).ok_or(Error::Overflow)?;
        let ticket = Ticket {
            id,
            generation: self.generation,
        };
        let mut requested_name = [0; MAX_NAME];
        requested_name[..name.len()].copy_from_slice(name.as_bytes());
        *slot = Some(Pending {
            ticket,
            client,
            deadline,
            name: requested_name,
            name_len: name.len(),
        });
        self.last_id = id;
        Ok(Lookup { ticket, name })
    }

    /// Consumes the ticket only on a valid reply and returns its intended recipient.
    pub fn respond(&mut self, sender: Peer, reply: &Reply<'_>, now: u64) -> Result<Peer, Error> {
        if self.backend != Some(sender) {
            return Err(Error::Denied);
        }
        reply.validate()?;
        let slot = self
            .pending
            .iter_mut()
            .find(|slot| slot.is_some_and(|request| request.ticket == reply.ticket))
            .ok_or(Error::Stale)?;
        let request = slot.unwrap();
        if now >= request.deadline {
            return Err(Error::Timeout);
        }
        if reply.status == Status::Found
            && reply
                .payload
                .split_once('\t')
                .map(|(name, _)| name.as_bytes())
                != Some(&request.name[..request.name_len])
        {
            return Err(Error::Malformed);
        }
        *slot = None;
        Ok(request.client)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACKEND: Peer = Peer {
        pid: 1,
        generation: 1,
    };
    const CLIENT: Peer = Peer {
        pid: 2,
        generation: 1,
    };

    #[test]
    fn frames_round_trip_and_reject_wrong_version_and_size() {
        let mut broker = Broker::<1>::new(true, 10);
        broker.register(BACKEND, true).unwrap();
        let lookup = broker.lookup(CLIENT, "alice", 0).unwrap();
        let mut bytes = [0; 128];
        let size = lookup.encode(&mut bytes).unwrap().len();
        assert_eq!(Lookup::decode(&bytes[..size]), Ok(lookup));
        assert_eq!(Lookup::decode(&bytes[..size - 1]), Err(Error::Malformed));
        bytes[0] = 2;
        assert_eq!(Lookup::decode(&bytes[..size]), Err(Error::Malformed));

        let mut reply = Reply {
            ticket: lookup.ticket,
            status: Status::Found,
            ttl_ms: 500,
            payload: "alice\t1000\t1000\t1001",
        };
        assert_eq!(Reply::decode(reply.encode(&mut bytes).unwrap()), Ok(reply));
        reply.payload = "alice\t4294967295\t1000";
        assert_eq!(reply.encode(&mut bytes), Err(Error::Malformed));
        reply.payload = "alice\t1000\t1000\t/etc/shadow";
        assert_eq!(reply.encode(&mut bytes), Err(Error::Malformed));
    }

    #[test]
    fn lifecycle_denies_forgery_and_replay_and_cancels_pending_requests() {
        assert_eq!(
            Broker::<1>::new(false, 10).register(BACKEND, true),
            Err(Error::Disabled)
        );
        let mut broker = Broker::<1>::new(true, 10);
        assert_eq!(broker.register(BACKEND, false), Err(Error::Denied));
        assert_eq!(broker.lookup(CLIENT, "alice", 0), Err(Error::Unavailable));
        broker.register(BACKEND, true).unwrap();
        let lookup = broker.lookup(CLIENT, "alice", 0).unwrap();
        let reply = Reply {
            ticket: lookup.ticket,
            status: Status::NotFound,
            ttl_ms: 5,
            payload: "",
        };
        assert_eq!(broker.lookup(CLIENT, "bob", 0), Err(Error::Busy));
        assert_eq!(broker.respond(CLIENT, &reply, 0), Err(Error::Denied));
        let mismatched = Reply {
            status: Status::Found,
            payload: "bob\t1001\t1001",
            ..reply
        };
        assert_eq!(
            broker.respond(BACKEND, &mismatched, 0),
            Err(Error::Malformed)
        );
        assert_eq!(broker.respond(BACKEND, &reply, 0), Ok(CLIENT));
        assert_eq!(broker.respond(BACKEND, &reply, 0), Err(Error::Stale));

        let timeout = broker.lookup(CLIENT, "alice", 5).unwrap().ticket;
        assert_eq!(
            broker.respond(
                BACKEND,
                &Reply {
                    ticket: timeout,
                    ..reply
                },
                15
            ),
            Err(Error::Timeout)
        );
        let mut cancelled = None;
        broker.expire(15, |peer, ticket, error| {
            cancelled = Some((peer, ticket, error))
        });
        assert_eq!(cancelled, Some((CLIENT, timeout, Error::Timeout)));

        let abandoned = broker.lookup(CLIENT, "alice", 15).unwrap().ticket;
        broker.disconnect(BACKEND, |peer, ticket, error| {
            cancelled = Some((peer, ticket, error))
        });
        assert_eq!(cancelled, Some((CLIENT, abandoned, Error::Unavailable)));
        let restarted = Peer {
            generation: 2,
            ..BACKEND
        };
        assert!(broker.register(restarted, true).unwrap() > abandoned.generation);
        assert_eq!(
            broker.respond(
                restarted,
                &Reply {
                    ticket: abandoned,
                    ..reply
                },
                15
            ),
            Err(Error::Stale)
        );
    }
}

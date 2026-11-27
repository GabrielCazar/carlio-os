//! Bounded anonymous pipes for the Linux descriptor ABI.
use crate::linux_abi::{EAGAIN, EBADF, EMFILE, EPIPE, POLLERR, POLLHUP, POLLIN, POLLOUT};

const FIRST_FD: u64 = 208;
const SLOTS: usize = 16;
const DESCRIPTORS: usize = 48;
pub const CAPACITY: usize = 4096;

pub fn is_descriptor(fd: u64) -> bool {
    (FIRST_FD..FIRST_FD + (DESCRIPTORS as u64)).contains(&fd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socketpair_shutdown_shares_state_across_aliases_and_keeps_reverse_stream() {
        use crate::linux_abi::POLLRDHUP;
        let mut table = PipeTable::new();
        let [left, right] = table.socketpair(1, false, false).unwrap().map(u64::from);
        let alias = table.duplicate(1, left, 0, false).unwrap();
        table.write(1, left, b"a").unwrap();
        table.shutdown(1, alias, 1).unwrap();
        assert_eq!(table.write(1, left, b"x"), Err(EPIPE));
        assert_eq!(
            table.poll(1, right, POLLIN | POLLRDHUP),
            Ok(POLLIN | POLLRDHUP)
        );
        let mut byte = [0];
        assert_eq!(table.read(1, right, &mut byte), Ok(1));
        assert_eq!(&byte, b"a");
        assert_eq!(table.read(1, right, &mut byte), Ok(0));
        assert_eq!(table.write(1, right, b"b"), Ok(1));
        table.shutdown(1, left, 0).unwrap();
        assert_eq!(table.write(1, right, b"x"), Err(EPIPE));
        assert_eq!(table.read(1, alias, &mut byte), Ok(1));
        assert_eq!(&byte, b"b");
    }

    #[test]
    fn socketpair_keeps_independent_streams_and_drains_after_peer_close() {
        let mut table = PipeTable::new();
        let [left, right] = table.socketpair(1, false, true).unwrap().map(u64::from);
        assert!(table.is_socket(1, left));
        assert_eq!(table.write(1, left, b"L"), Ok(1));
        assert_eq!(table.write(1, right, b"R"), Ok(1));
        let mut buffer = [0];
        assert_eq!(table.read(1, left, &mut buffer), Ok(1));
        assert_eq!(&buffer, b"R");
        table.close(1, left).unwrap();
        assert_eq!(table.poll(1, right, POLLIN), Ok(POLLIN | POLLHUP));
        assert_eq!(table.read(1, right, &mut buffer), Ok(1));
        assert_eq!(&buffer, b"L");
        assert_eq!(table.read(1, right, &mut buffer), Ok(0));
        assert_eq!(table.write(1, right, b"x"), Err(EPIPE));
    }

    #[test]
    fn exact_dup_replaces_only_the_callers_target() {
        let mut table = PipeTable::new();
        let [read, write] = table.create(1, false, false).unwrap().map(u64::from);
        let [other_read, other_write] = table.create(2, false, false).unwrap().map(u64::from);
        assert_eq!(other_read, read);
        table.duplicate_to(1, write, 1, true).unwrap();
        table.duplicate_to(1, read, 1, false).unwrap();
        table.close(1, read).unwrap();
        assert_eq!(table.write(1, write, b"a"), Ok(1));
        assert_eq!(table.read(1, 1, &mut [0]), Ok(1));
        assert_eq!(table.write(2, other_write, b"b"), Ok(1));
        let mut output = [0];
        assert_eq!(table.read(2, other_read, &mut output), Ok(1));
        assert_eq!(&output, b"b");
    }

    #[test]
    fn aliases_share_status_and_keep_the_pipe_open_until_last_close() {
        use crate::linux_abi::DescriptorControl::*;

        let mut table = PipeTable::new();
        let [read, write] = table.create(1, false, true).unwrap().map(u64::from);
        let alias = table.duplicate(1, write, 230, false).unwrap();
        assert_eq!(alias, 230);
        assert_eq!(table.duplicate(2, write, 0, false), Err(EBADF));
        assert_eq!(table.fcntl(1, alias, GetDescriptorFlags), Ok(0));
        assert_eq!(table.fcntl(1, write, GetDescriptorFlags), Ok(1));
        table.fcntl(1, alias, SetNonblocking(true)).unwrap();
        assert_eq!(table.nonblocking(1, write), Ok(true));
        table.close(1, write).unwrap();
        assert_eq!(table.poll(1, read, POLLIN), Ok(0));
        assert_eq!(table.write(1, alias, b"x"), Ok(1));
        table.close(1, alias).unwrap();
        assert_eq!(table.poll(1, read, POLLIN), Ok(POLLIN | POLLHUP));
    }

    #[test]
    fn pipe_preserves_atomic_writes_and_reports_eof_after_draining() {
        let mut table = PipeTable::new();
        let [read, write] = table.create(1, true, false).unwrap().map(u64::from);
        assert_eq!(table.read(1, read, &mut [0]), Err(EAGAIN));
        assert_eq!(table.write(2, write, b"x"), Err(EBADF));
        assert_eq!(table.write(1, write, &[42; CAPACITY - 1]), Ok(CAPACITY - 1));
        assert_eq!(table.write(1, write, b"ab"), Err(EAGAIN));
        assert_eq!(table.poll_with_write_size(1, write, POLLOUT, 2), Ok(0));
        table.close(1, write).unwrap();
        assert_eq!(table.poll(1, read, POLLIN), Ok(POLLIN | POLLHUP));
        assert_eq!(table.read(1, read, &mut [0; CAPACITY]), Ok(CAPACITY - 1));
        assert_eq!(table.read(1, read, &mut [0]), Ok(0));
    }

    #[test]
    fn pipe_exec_cleanup_and_broken_reader() {
        use crate::linux_abi::DescriptorControl;
        let mut table = PipeTable::new();
        let [read, write] = table.create(1, false, false).unwrap().map(u64::from);
        let [exec_read, _] = table.create(1, false, true).unwrap().map(u64::from);
        table
            .fcntl(1, read, DescriptorControl::SetCloseOnExec(true))
            .unwrap();
        table
            .fcntl(1, write, DescriptorControl::SetNonblocking(true))
            .unwrap();
        assert_eq!(table.nonblocking(1, read), Ok(false));
        assert_eq!(
            table.fcntl(1, write, DescriptorControl::GetStatusFlags),
            Ok(0x801)
        );
        table.close_process(1, true);
        assert_eq!(table.poll(1, exec_read, POLLIN), Err(EBADF));
        assert_eq!(table.poll(1, read, POLLIN), Err(EBADF));
        assert_eq!(table.poll(1, write, 0), Ok(POLLERR));
        assert_eq!(table.write(1, write, b"x"), Err(EPIPE));
    }
}

#[derive(Clone, Copy)]
struct PipeDescriptor {
    owner: u64,
    fd: u64,
    pipe: usize,
    end: usize,
    cloexec: bool,
}

struct Pipe {
    references: [usize; 2],
    nonblocking: [bool; 2],
    duplex: bool,
    read_shutdown: [bool; 2],
    write_shutdown: [bool; 2],
    bytes: [[u8; CAPACITY]; 2],
    len: [usize; 2],
}

impl Pipe {
    const EMPTY: Self = Self {
        references: [0; 2],
        nonblocking: [false; 2],
        duplex: false,
        read_shutdown: [false; 2],
        write_shutdown: [false; 2],
        bytes: [[0; CAPACITY]; 2],
        len: [0; 2],
    };
}

pub struct PipeTable {
    entries: [Pipe; SLOTS],
    descriptors: [Option<PipeDescriptor>; DESCRIPTORS],
}

impl PipeTable {
    pub const fn new() -> Self {
        Self {
            entries: [const { Pipe::EMPTY }; SLOTS],
            descriptors: [None; DESCRIPTORS],
        }
    }

    pub fn create(
        &mut self,
        owner: u64,
        nonblocking: bool,
        cloexec: bool,
    ) -> Result<[u32; 2], u16> {
        self.create_channel(owner, nonblocking, cloexec, false)
    }

    pub fn socketpair(
        &mut self,
        owner: u64,
        nonblocking: bool,
        cloexec: bool,
    ) -> Result<[u32; 2], u16> {
        self.create_channel(owner, nonblocking, cloexec, true)
    }

    fn create_channel(
        &mut self,
        owner: u64,
        nonblocking: bool,
        cloexec: bool,
        duplex: bool,
    ) -> Result<[u32; 2], u16> {
        let index = self
            .entries
            .iter()
            .position(|pipe| pipe.references == [0; 2])
            .ok_or(EMFILE)?;
        let mut free = self
            .descriptors
            .iter()
            .enumerate()
            .filter(|(_, descriptor)| descriptor.is_none())
            .map(|(slot, _)| slot);
        let slots = [free.next().ok_or(EMFILE)?, free.next().ok_or(EMFILE)?];
        let mut numbers =
            (FIRST_FD..FIRST_FD + DESCRIPTORS as u64).filter(|&fd| !self.contains(owner, fd));
        let fds = [numbers.next().ok_or(EMFILE)?, numbers.next().ok_or(EMFILE)?];
        let pipe = &mut self.entries[index];
        pipe.references = [1; 2];
        pipe.nonblocking = [nonblocking; 2];
        pipe.duplex = duplex;
        pipe.read_shutdown = [false; 2];
        pipe.write_shutdown = [false; 2];
        pipe.len = [0; 2];
        for (end, slot) in slots.into_iter().enumerate() {
            self.descriptors[slot] = Some(PipeDescriptor {
                owner,
                fd: fds[end],
                pipe: index,
                end,
                cloexec,
            });
        }
        Ok(fds.map(|fd| fd as u32))
    }

    fn descriptor_slot(&self, owner: u64, fd: u64) -> Result<usize, u16> {
        self.descriptors
            .iter()
            .position(|entry| entry.is_some_and(|entry| entry.owner == owner && entry.fd == fd))
            .ok_or(EBADF)
    }

    pub fn is_socket(&self, owner: u64, fd: u64) -> bool {
        self.locate(owner, fd)
            .is_ok_and(|(index, _)| self.entries[index].duplex)
    }

    pub fn contains(&self, owner: u64, fd: u64) -> bool {
        self.descriptor_slot(owner, fd).is_ok()
    }

    pub fn inherit(&mut self, parent: u64, child: u64) -> Result<(), u16> {
        if parent == child
            || self
                .descriptors
                .iter()
                .flatten()
                .any(|entry| entry.owner == child)
        {
            return Err(crate::linux_abi::EINVAL);
        }
        let count = self
            .descriptors
            .iter()
            .flatten()
            .filter(|entry| entry.owner == parent)
            .count();
        if self
            .descriptors
            .iter()
            .filter(|entry| entry.is_none())
            .count()
            < count
        {
            return Err(EMFILE);
        }
        for slot in 0..DESCRIPTORS {
            let Some(mut entry) = self.descriptors[slot].filter(|entry| entry.owner == parent)
            else {
                continue;
            };
            let target = self.descriptors.iter().position(Option::is_none).unwrap();
            entry.owner = child;
            self.entries[entry.pipe].references[entry.end] += 1;
            self.descriptors[target] = Some(entry);
        }
        Ok(())
    }

    fn locate(&self, owner: u64, fd: u64) -> Result<(usize, usize), u16> {
        let entry = self.descriptors[self.descriptor_slot(owner, fd)?].ok_or(EBADF)?;
        Ok((entry.pipe, entry.end))
    }

    pub fn nonblocking(&self, owner: u64, fd: u64) -> Result<bool, u16> {
        let (index, end) = self.locate(owner, fd)?;
        Ok(self.entries[index].nonblocking[end])
    }

    pub fn duplicate(
        &mut self,
        owner: u64,
        fd: u64,
        minimum: u64,
        cloexec: bool,
    ) -> Result<u64, u16> {
        let source = self.descriptor_slot(owner, fd)?;
        let first = minimum.max(FIRST_FD);
        let limit = FIRST_FD + DESCRIPTORS as u64;
        if first >= limit {
            return Err(crate::linux_abi::EINVAL);
        }
        let fd = (first..limit)
            .find(|&fd| !self.contains(owner, fd))
            .ok_or(EMFILE)?;
        self.install_alias(source, fd, cloexec)
    }

    pub fn duplicate_to(
        &mut self,
        owner: u64,
        fd: u64,
        target: u64,
        cloexec: bool,
    ) -> Result<u64, u16> {
        let source = self.descriptor_slot(owner, fd)?;
        if fd == target {
            return Ok(fd);
        }
        if target >= crate::dircache_socket::FIRST_FD && !is_descriptor(target) {
            return Err(crate::linux_abi::EOPNOTSUPP);
        }
        self.install_alias(source, target, cloexec)
    }

    fn install_alias(&mut self, source: usize, fd: u64, cloexec: bool) -> Result<u64, u16> {
        let mut entry = self.descriptors[source].ok_or(EBADF)?;
        let target = match self.descriptor_slot(entry.owner, fd) {
            Ok(slot) => slot,
            Err(_) => self
                .descriptors
                .iter()
                .position(Option::is_none)
                .ok_or(EMFILE)?,
        };
        self.entries[entry.pipe].references[entry.end] += 1;
        self.close_slot(target);
        entry.fd = fd;
        entry.cloexec = cloexec;
        self.descriptors[target] = Some(entry);
        Ok(fd)
    }

    pub fn fcntl(
        &mut self,
        owner: u64,
        fd: u64,
        operation: crate::linux_abi::DescriptorControl,
    ) -> Result<u64, u16> {
        let slot = self.descriptor_slot(owner, fd)?;
        let entry = self.descriptors[slot].as_mut().ok_or(EBADF)?;
        Ok(operation.apply(
            if self.entries[entry.pipe].duplex {
                2
            } else {
                entry.end as u64
            },
            &mut entry.cloexec,
            &mut self.entries[entry.pipe].nonblocking[entry.end],
        ))
    }

    pub fn shutdown(&mut self, owner: u64, fd: u64, how: u32) -> Result<(), u16> {
        use crate::linux_abi::{EINVAL, ENOTSOCK};
        let (index, end) = self.locate(owner, fd)?;
        let pipe = &mut self.entries[index];
        if !pipe.duplex {
            return Err(ENOTSOCK);
        }
        if how > 2 {
            return Err(EINVAL);
        }
        if how != 1 {
            pipe.read_shutdown[end] = true;
        }
        if how != 0 {
            pipe.write_shutdown[end] = true;
        }
        Ok(())
    }

    pub fn read(&mut self, owner: u64, fd: u64, output: &mut [u8]) -> Result<usize, u16> {
        let (index, end) = self.locate(owner, fd)?;
        let pipe = &mut self.entries[index];
        if end != 0 && !pipe.duplex {
            return Err(EBADF);
        }
        let length = pipe.len[end];
        let eof = pipe.references[1 - end] == 0
            || pipe.read_shutdown[end]
            || pipe.write_shutdown[1 - end];
        if output.is_empty() || (length == 0 && eof) {
            return Ok(0);
        }
        if length == 0 {
            return Err(EAGAIN);
        }
        let amount = length.min(output.len());
        output[..amount].copy_from_slice(&pipe.bytes[end][..amount]);
        pipe.bytes[end].copy_within(amount..length, 0);
        pipe.len[end] -= amount;
        Ok(amount)
    }

    pub fn write(&mut self, owner: u64, fd: u64, input: &[u8]) -> Result<usize, u16> {
        let (index, end) = self.locate(owner, fd)?;
        let pipe = &mut self.entries[index];
        if end != 1 && !pipe.duplex {
            return Err(EBADF);
        }
        if input.is_empty() {
            return Ok(0);
        }
        let peer = 1 - end;
        if pipe.references[peer] == 0 || pipe.write_shutdown[end] || pipe.read_shutdown[peer] {
            return Err(EPIPE);
        }
        let offset = pipe.len[peer];
        let amount = input.len().min(CAPACITY - offset);
        if amount == 0 || (!pipe.duplex && input.len() <= CAPACITY && amount < input.len()) {
            return Err(EAGAIN);
        }
        pipe.bytes[peer][offset..offset + amount].copy_from_slice(&input[..amount]);
        pipe.len[peer] += amount;
        Ok(amount)
    }

    pub fn poll(&self, owner: u64, fd: u64, events: u16) -> Result<u16, u16> {
        self.poll_with_write_size(owner, fd, events, 1)
    }

    pub fn poll_with_write_size(
        &self,
        owner: u64,
        fd: u64,
        events: u16,
        needed: usize,
    ) -> Result<u16, u16> {
        let (index, end) = self.locate(owner, fd)?;
        let pipe = &self.entries[index];
        let peer = 1 - end;
        let read_closed =
            pipe.references[peer] == 0 || pipe.read_shutdown[end] || pipe.write_shutdown[peer];
        let write_closed =
            pipe.references[peer] == 0 || pipe.write_shutdown[end] || pipe.read_shutdown[peer];
        if pipe.duplex {
            use crate::linux_abi::POLLRDHUP;
            let mut ready = 0;
            if read_closed {
                ready |= events & (POLLIN | POLLRDHUP);
            } else if pipe.len[end] != 0 {
                ready |= events & POLLIN;
            }
            if write_closed || pipe.len[peer] < CAPACITY {
                ready |= events & POLLOUT;
            }
            if read_closed && write_closed {
                ready |= POLLHUP;
            }
            return Ok(ready);
        }
        let mut ready = 0;
        if end == 0 {
            if pipe.len[end] != 0 {
                ready |= events & POLLIN;
            }
            if pipe.references[peer] == 0 {
                ready |= POLLHUP;
            }
        } else {
            if CAPACITY - pipe.len[peer] >= needed {
                ready |= events & POLLOUT;
            }
            if pipe.references[peer] == 0 {
                ready |= POLLERR;
            }
        }
        Ok(ready)
    }

    pub fn close(&mut self, owner: u64, fd: u64) -> Result<(), u16> {
        let slot = self.descriptor_slot(owner, fd)?;
        self.close_slot(slot);
        Ok(())
    }

    fn close_slot(&mut self, slot: usize) {
        if let Some(entry) = self.descriptors[slot].take() {
            self.entries[entry.pipe].references[entry.end] -= 1;
        }
    }

    pub fn close_process(&mut self, owner: u64, exec: bool) {
        for slot in 0..DESCRIPTORS {
            if self.descriptors[slot]
                .is_some_and(|entry| entry.owner == owner && (!exec || entry.cloexec))
            {
                self.close_slot(slot);
            }
        }
    }
}

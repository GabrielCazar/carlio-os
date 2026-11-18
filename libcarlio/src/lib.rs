#![forbid(unsafe_code)]
#![no_std]

//! Safe request interface for CarlioOS ABI v1.
//!
//! This crate intentionally has no raw syscall instruction yet. A target-side
//! adapter must be implemented and validated before applications can use this
//! interface to enter the kernel. Borrowed buffers keep raw user pointers out
//! of the API. See `docs/KERNEL_ABI.md` for the frozen syscall contract.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error(pub i32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fd(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mapping {
    pub address: u64,
    pub length: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timespec {
    pub seconds: i64,
    pub nanoseconds: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Statx {
    pub mask: u32,
    pub mode: u16,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub inode: u64,
    pub mtime: Timespec,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub fd: Fd,
    pub events: u32,
    pub user_data: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CloneArgs {
    pub flags: u64,
    pub exit_signal: u64,
    pub stack_size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clock {
    Realtime,
    Monotonic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalAction<'a> {
    Set {
        signal: u8,
        handler: u64,
        mask: &'a [u64],
    },
    Block {
        mask: &'a [u64],
    },
    Return,
    SendThread {
        tid: u64,
        signal: u8,
    },
}

/// Implemented by a syscall transport or a deterministic conformance harness.
/// Every buffer is borrowed and bounded by its Rust slice length.
pub trait KernelAbi {
    fn openat(&self, dirfd: i32, path: &[u8], flags: u32, mode: u32) -> Result<Fd, Error>;
    fn read(&self, fd: Fd, output: &mut [u8]) -> Result<usize, Error>;
    fn write(&self, fd: Fd, input: &[u8]) -> Result<usize, Error>;
    fn close(&self, fd: Fd) -> Result<(), Error>;
    fn fsync(&self, fd: Fd) -> Result<(), Error>;
    fn renameat2(
        &self,
        old_dir: i32,
        old: &[u8],
        new_dir: i32,
        new: &[u8],
        flags: u32,
    ) -> Result<(), Error>;
    fn statx(&self, dirfd: i32, path: &[u8], flags: u32, mask: u32) -> Result<Statx, Error>;
    fn mmap(
        &self,
        hint: u64,
        length: u64,
        prot: u32,
        flags: u32,
        fd: i32,
        offset: u64,
    ) -> Result<Mapping, Error>;
    fn munmap(&self, mapping: Mapping) -> Result<(), Error>;
    fn mprotect(&self, mapping: Mapping, prot: u32) -> Result<(), Error>;
    fn clone(&self, args: CloneArgs) -> Result<u64, Error>;
    fn futex_wait(
        &self,
        word: &core::sync::atomic::AtomicU32,
        expected: u32,
        timeout: Option<Timespec>,
    ) -> Result<(), Error>;
    fn futex_wake(&self, word: &core::sync::atomic::AtomicU32, count: u32) -> Result<u32, Error>;
    fn execve(&self, path: &[u8], argv: &[&[u8]], envp: &[&[u8]]) -> Result<(), Error>;
    fn signal(&self, action: SignalAction<'_>) -> Result<(), Error>;
    fn getrandom(&self, output: &mut [u8], flags: u32) -> Result<usize, Error>;
    fn clock_gettime(&self, clock: Clock) -> Result<Timespec, Error>;
    fn epoll_create1(&self, flags: u32) -> Result<Fd, Error>;
    fn epoll_ctl(
        &self,
        epoll: Fd,
        operation: u32,
        fd: Fd,
        event: Option<Event>,
    ) -> Result<(), Error>;
    fn epoll_wait(&self, epoll: Fd, events: &mut [Event], timeout_ms: i32) -> Result<usize, Error>;
    fn socket(&self, domain: i32, kind: i32, protocol: i32) -> Result<Fd, Error>;
    fn bind(&self, fd: Fd, address: &[u8]) -> Result<(), Error>;
    fn connect(&self, fd: Fd, address: &[u8]) -> Result<(), Error>;
    fn listen(&self, fd: Fd, backlog: u32) -> Result<(), Error>;
    fn accept4(&self, fd: Fd, address: &mut [u8], flags: u32) -> Result<(Fd, usize), Error>;
    fn send(&self, fd: Fd, bytes: &[u8], flags: u32) -> Result<usize, Error>;
    fn recv(&self, fd: Fd, bytes: &mut [u8], flags: u32) -> Result<usize, Error>;
    fn send_rights(&self, fd: Fd, bytes: &[u8], rights: &[Fd]) -> Result<usize, Error>;
    fn recv_rights(
        &self,
        fd: Fd,
        bytes: &mut [u8],
        rights: &mut [Fd],
    ) -> Result<(usize, usize), Error>;
    fn set_reuseport(&self, fd: Fd, enabled: bool) -> Result<(), Error>;
    fn sendfile(
        &self,
        output: Fd,
        input: Fd,
        offset: Option<u64>,
        count: u64,
    ) -> Result<u64, Error>;
    fn splice(&self, input: Fd, output: Fd, count: u64, flags: u32) -> Result<u64, Error>;
}

/// Safe facade used by services; it does not claim a kernel transport exists.
pub struct Client<A> {
    abi: A,
}

impl<A: KernelAbi> Client<A> {
    pub const fn new(abi: A) -> Self {
        Self { abi }
    }
    pub fn abi(&self) -> &A {
        &self.abi
    }
    pub fn openat(&self, d: i32, p: &[u8], f: u32, m: u32) -> Result<Fd, Error> {
        self.abi.openat(d, p, f, m)
    }
    pub fn read(&self, fd: Fd, out: &mut [u8]) -> Result<usize, Error> {
        self.abi.read(fd, out)
    }
    pub fn write(&self, fd: Fd, input: &[u8]) -> Result<usize, Error> {
        self.abi.write(fd, input)
    }
    pub fn close(&self, fd: Fd) -> Result<(), Error> {
        self.abi.close(fd)
    }
    pub fn fsync(&self, fd: Fd) -> Result<(), Error> {
        self.abi.fsync(fd)
    }
    pub fn renameat2(&self, od: i32, o: &[u8], nd: i32, n: &[u8], f: u32) -> Result<(), Error> {
        self.abi.renameat2(od, o, nd, n, f)
    }
    pub fn statx(&self, d: i32, p: &[u8], f: u32, m: u32) -> Result<Statx, Error> {
        self.abi.statx(d, p, f, m)
    }
    pub fn mmap(&self, h: u64, l: u64, p: u32, f: u32, fd: i32, o: u64) -> Result<Mapping, Error> {
        self.abi.mmap(h, l, p, f, fd, o)
    }
    pub fn munmap(&self, m: Mapping) -> Result<(), Error> {
        self.abi.munmap(m)
    }
    pub fn mprotect(&self, m: Mapping, p: u32) -> Result<(), Error> {
        self.abi.mprotect(m, p)
    }
    pub fn clone(&self, a: CloneArgs) -> Result<u64, Error> {
        self.abi.clone(a)
    }
    pub fn futex_wait(
        &self,
        w: &core::sync::atomic::AtomicU32,
        e: u32,
        t: Option<Timespec>,
    ) -> Result<(), Error> {
        self.abi.futex_wait(w, e, t)
    }
    pub fn futex_wake(&self, w: &core::sync::atomic::AtomicU32, c: u32) -> Result<u32, Error> {
        self.abi.futex_wake(w, c)
    }
    pub fn execve(&self, p: &[u8], a: &[&[u8]], e: &[&[u8]]) -> Result<(), Error> {
        self.abi.execve(p, a, e)
    }
    pub fn signal(&self, a: SignalAction<'_>) -> Result<(), Error> {
        self.abi.signal(a)
    }
    pub fn getrandom(&self, o: &mut [u8], f: u32) -> Result<usize, Error> {
        self.abi.getrandom(o, f)
    }
    pub fn clock_gettime(&self, c: Clock) -> Result<Timespec, Error> {
        self.abi.clock_gettime(c)
    }
    pub fn epoll_create1(&self, f: u32) -> Result<Fd, Error> {
        self.abi.epoll_create1(f)
    }
    pub fn epoll_ctl(&self, e: Fd, op: u32, fd: Fd, event: Option<Event>) -> Result<(), Error> {
        self.abi.epoll_ctl(e, op, fd, event)
    }
    pub fn epoll_wait(&self, e: Fd, out: &mut [Event], t: i32) -> Result<usize, Error> {
        self.abi.epoll_wait(e, out, t)
    }
    pub fn socket(&self, d: i32, k: i32, p: i32) -> Result<Fd, Error> {
        self.abi.socket(d, k, p)
    }
    pub fn bind(&self, fd: Fd, a: &[u8]) -> Result<(), Error> {
        self.abi.bind(fd, a)
    }
    pub fn connect(&self, fd: Fd, a: &[u8]) -> Result<(), Error> {
        self.abi.connect(fd, a)
    }
    pub fn listen(&self, fd: Fd, b: u32) -> Result<(), Error> {
        self.abi.listen(fd, b)
    }
    pub fn accept4(&self, fd: Fd, a: &mut [u8], f: u32) -> Result<(Fd, usize), Error> {
        self.abi.accept4(fd, a, f)
    }
    pub fn send(&self, fd: Fd, b: &[u8], f: u32) -> Result<usize, Error> {
        self.abi.send(fd, b, f)
    }
    pub fn recv(&self, fd: Fd, b: &mut [u8], f: u32) -> Result<usize, Error> {
        self.abi.recv(fd, b, f)
    }
    pub fn send_rights(&self, fd: Fd, b: &[u8], rights: &[Fd]) -> Result<usize, Error> {
        self.abi.send_rights(fd, b, rights)
    }
    pub fn recv_rights(
        &self,
        fd: Fd,
        b: &mut [u8],
        rights: &mut [Fd],
    ) -> Result<(usize, usize), Error> {
        self.abi.recv_rights(fd, b, rights)
    }
    pub fn set_reuseport(&self, fd: Fd, on: bool) -> Result<(), Error> {
        self.abi.set_reuseport(fd, on)
    }
    pub fn sendfile(&self, o: Fd, i: Fd, off: Option<u64>, n: u64) -> Result<u64, Error> {
        self.abi.sendfile(o, i, off, n)
    }
    pub fn splice(&self, i: Fd, o: Fd, n: u64, f: u32) -> Result<u64, Error> {
        self.abi.splice(i, o, n, f)
    }
}

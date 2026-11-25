//! Fatia inicial da ABI Linux x86_64, compartilhada entre as bases Arch/Ubuntu.
//!
//! O dispatcher mantém somente syscalls de console e ciclo de vida que podem
//! ser implementadas com os serviços explicitamente fornecidos pelo chamador.
//! Parte das chamadas entra pelo opcode `SYSCALL`; VFS e semânticas gerais
//! continuam sendo conectados por etapas.

pub const SYS_WRITE: u64 = 1;
pub const SYS_GETRANDOM: u64 = 318;
pub const SYS_STATX: u64 = 332;
pub const SYS_PREAD64: u64 = 17;
pub const SYS_WRITEV: u64 = 20;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_FSYNC: u64 = 74;
pub const SYS_LSEEK: u64 = 8;
pub const SYS_MMAP: u64 = 9;
pub const SYS_MPROTECT: u64 = 10;
pub const SYS_MUNMAP: u64 = 11;
pub const SYS_MADVISE: u64 = 28;
pub const MADV_PAGEOUT: u64 = 21;
pub const SYS_BRK: u64 = 12;
pub const SYS_READ: u64 = 0;
pub const SYS_OPEN: u64 = 2;
pub const SYS_FSTAT: u64 = 5;
pub const SYS_POLL: u64 = 7;
pub const SYS_RT_SIGACTION: u64 = 13;
pub const POLLIN: u16 = 0x001;
pub const POLLOUT: u16 = 0x004;
pub const POLLERR: u16 = 0x008;
pub const POLLNVAL: u16 = 0x020;
pub const SIGPIPE: u64 = 13;
pub const SYS_ACCESS: u64 = 21;
pub const SYS_NEWFSTATAT: u64 = 262;
pub const SYS_GETPID: u64 = 39;
pub const SYS_UNAME: u64 = 63;
pub const SYS_OPENAT: u64 = 257;
pub const SYS_GETUID: u64 = 102;
pub const SYS_GETGID: u64 = 104;
pub const SYS_GETEUID: u64 = 107;
pub const SYS_GETEGID: u64 = 108;
pub const SYS_GETGROUPS: u64 = 115;
pub const SYS_EXIT: u64 = 60;
pub const SYS_EXECVE: u64 = 59;
pub const SYS_EXIT_GROUP: u64 = 231;
pub const SYS_SOCKET: u64 = 41;
pub const SYS_CONNECT: u64 = 42;
pub const SYS_ACCEPT: u64 = 43;
pub const SYS_SENDTO: u64 = 44;
pub const SYS_RECVFROM: u64 = 45;
pub const SYS_ACCEPT4: u64 = 288;
pub const SYS_BIND: u64 = 49;
pub const SYS_LISTEN: u64 = 50;
pub const SYS_GETSOCKNAME: u64 = 51;
pub const SYS_CLOCK_GETTIME: u64 = 228;
pub const SYS_CLOCK_GETRES: u64 = 229;
pub const SYS_ARCH_PRCTL: u64 = 158;
pub const ARCH_SET_FS: u64 = 0x1002;
pub const ARCH_GET_FS: u64 = 0x1003;
pub const SYS_GETTID: u64 = 186;
pub const SYS_GETCWD: u64 = 79;
pub const SYS_MKDIR: u64 = 83;
pub const SYS_UMASK: u64 = 95;
pub const SYS_CHMOD: u64 = 90;
pub const SYS_FCHMOD: u64 = 91;
pub const SYS_CHOWN: u64 = 92;
pub const SYS_FCHOWN: u64 = 93;
pub const SYS_LCHOWN: u64 = 94;
pub const SYS_FCHOWNAT: u64 = 260;
pub const SYS_FCHMODAT: u64 = 268;
pub const SYS_UNLINK: u64 = 87;
pub const SYS_UNLINKAT: u64 = 263;
pub const SYS_RMDIR: u64 = 84;
pub const AT_EMPTY_PATH: u64 = 0x1000;
pub const AT_REMOVEDIR: u64 = 0x200;
pub const AT_SYMLINK_NOFOLLOW: u64 = 0x100;

pub const CLOCK_REALTIME: u64 = 0;
pub const CLOCK_MONOTONIC: u64 = 1;

pub const EBADF: u16 = 9;
pub const E2BIG: u16 = 7;
pub const EPERM: u16 = 1;
pub const ENOENT: u16 = 2;
pub const ENOEXEC: u16 = 8;
pub const ENOTDIR: u16 = 20;
pub const ENOTEMPTY: u16 = 39;
pub const EFAULT: u16 = 14;
pub const EIO: u16 = 5;
pub const EINVAL: u16 = 22;
pub const EISDIR: u16 = 21;
pub const EFBIG: u16 = 27;
pub const ENOMEM: u16 = 12;
pub const ENOSYS: u16 = 38;
pub const EACCES: u16 = 13;
pub const EAFNOSUPPORT: u16 = 97;
pub const EADDRINUSE: u16 = 98;
pub const EADDRNOTAVAIL: u16 = 99;
pub const ENFILE: u16 = 23;
pub const EOPNOTSUPP: u16 = 95;
pub const EROFS: u16 = 30;
pub const EEXIST: u16 = 17;
pub const EOVERFLOW: u16 = 75;
pub const ERANGE: u16 = 34;
pub const ENOTSOCK: u16 = 88;
pub const ESPIPE: u16 = 29;
pub const EMSGSIZE: u16 = 90;
pub const ENOTCONN: u16 = 107;
pub const ETIMEDOUT: u16 = 110;
pub const ECONNREFUSED: u16 = 111;
pub const ECONNRESET: u16 = 104;
pub const EAGAIN: u16 = 11;
pub const EBUSY: u16 = 16;

const WRITE_CHUNK_SIZE: usize = 256;
const PATH_BUFFER_SIZE: usize = crate::vfs::VFS_PATH_MAX;
const EXEC_VECTOR_LIMIT: usize = 8;
const EXEC_STRING_LIMIT: usize = 256;
const O_ACCMODE: u64 = 3;
const O_WRONLY: u64 = 1;
const O_RDWR: u64 = 2;
const O_CREAT: u64 = 0x40;
const O_EXCL: u64 = 0x80;
const O_TRUNC: u64 = 0x200;
pub const O_CLOEXEC: u64 = 0x80000;
const O_APPEND: u64 = 0x400;
const AT_FDCWD: u64 = (-100i64) as u64;
/// Limita o tempo máximo gasto em uma única chamada síncrona de console.
pub const MAX_WRITE_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessContext {
    pub process_id: u64,
    /// ID único da thread atual; `gettid` pode divergir do PID do grupo.
    pub thread_id: u64,
    /// IDs POSIX efetivos e reais do processo no modelo atual.
    pub uid: u32,
    pub gid: u32,
    pub supplementary_groups: [u32; 8],
    pub group_count: u8,
    /// Break atual do processo; o backend valida crescimento contra suas VMA.
    pub program_break: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyscallOutcome {
    Return(u64),
    Exec(ExecTransition),
    Exit { status: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecTransition {
    pub process_id: u64,
    pub old_page_table: u64,
    pub old_resource_slot: u8,
    pub page_table: u64,
    pub instruction_pointer: u64,
    pub stack_pointer: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timespec {
    pub seconds: i64,
    pub nanoseconds: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinuxStat {
    pub inode: u64,
    pub mode: u32,
    pub links: u64,
    pub uid: u32,
    pub gid: u32,
    pub size: i64,
    pub block_size: i64,
    pub blocks: i64,
    pub access_time: Timespec,
    pub modification_time: Timespec,
    pub change_time: Timespec,
}

/// Serviços mínimos que o backend precisa fornecer sem expor ponteiros user
/// diretamente ao dispatcher. `write` deve aceitar somente descritores válidos.
pub trait LinuxSyscallIo {
    fn copy_from_user(&mut self, address: u64, destination: &mut [u8]) -> Result<(), u16>;
    /// Valida o buffer de saída inteiro antes que `read` consuma a origem.
    fn validate_user_write(&mut self, _address: u64, _length: usize) -> Result<(), u16> {
        Ok(())
    }
    fn copy_to_user(&mut self, _address: u64, _source: &[u8]) -> Result<(), u16> {
        Err(EFAULT)
    }
    fn read(&mut self, _descriptor: u64, _destination: &mut [u8]) -> Result<usize, u16> {
        Err(EBADF)
    }
    fn pread(
        &mut self,
        _descriptor: u64,
        _offset: u64,
        _destination: &mut [u8],
    ) -> Result<usize, u16> {
        Err(ENOSYS)
    }
    fn open(&mut self, _path: &str, _flags: u64, _mode: u64) -> Result<u64, u16> {
        Err(ENOSYS)
    }
    fn execve(
        &mut self,
        _path: &str,
        _arguments: &[&str],
        _environment: &[&str],
    ) -> Result<ExecTransition, u16> {
        Err(ENOSYS)
    }
    fn write(&mut self, descriptor: u64, bytes: &[u8]) -> Result<usize, u16>;
    fn close(&mut self, _descriptor: u64) -> Result<(), u16> {
        Err(EBADF)
    }
    fn fsync(&mut self, _descriptor: u64) -> Result<(), u16> {
        Err(EBADF)
    }
    fn fstat(&mut self, _descriptor: u64) -> Result<LinuxStat, u16> {
        Err(EBADF)
    }
    fn stat(&mut self, _path: &str) -> Result<LinuxStat, u16> {
        Err(ENOSYS)
    }
    fn random_bytes(&mut self, _destination: &mut [u8]) -> Result<(), u16> {
        Err(ENOSYS)
    }
    fn poll_events(&mut self, _descriptor: u32, _events: u16) -> Result<u16, u16> {
        Err(ENOSYS)
    }
    fn ignores_sigpipe(&self) -> bool {
        false
    }
    fn set_sigpipe_ignored(&mut self, _ignored: bool) -> Result<(), u16> {
        Err(ENOSYS)
    }
    fn access(&mut self, _path: &str, _mode: u64) -> Result<(), u16> {
        Err(ENOSYS)
    }
    fn mkdir(&mut self, _path: &str, _mode: u16) -> Result<(), u16> {
        Err(ENOSYS)
    }
    fn umask(&mut self, _mask: u16) -> Result<u16, u16> {
        Err(ENOSYS)
    }
    fn chmod(&mut self, _path: &str, _mode: u16) -> Result<(), u16> {
        Err(ENOSYS)
    }
    fn fchmod(&mut self, _descriptor: u64, _mode: u16) -> Result<(), u16> {
        Err(ENOSYS)
    }
    fn chown(&mut self, _path: &str, _uid: Option<u32>, _gid: Option<u32>) -> Result<(), u16> {
        Err(ENOSYS)
    }
    fn fchown(
        &mut self,
        _descriptor: u64,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> Result<(), u16> {
        Err(ENOSYS)
    }
    fn unlink(&mut self, _path: &str) -> Result<(), u16> {
        Err(ENOSYS)
    }
    fn rmdir(&mut self, _path: &str) -> Result<(), u16> {
        Err(ENOSYS)
    }
    fn socket(&mut self, _domain: u64, _kind: u64, _protocol: u64) -> Result<u64, u16> {
        Err(EOPNOTSUPP)
    }
    fn unix_address(&mut self, _fd: u64, _path: &[u8], _bind: bool) -> Result<(), u16> {
        Err(EOPNOTSUPP)
    }
    fn unix_name(&mut self, _fd: u64) -> Result<bool, u16> {
        Err(EBADF)
    }
    fn bind(&mut self, _descriptor: u64, _address: [u8; 4], _port: u16) -> Result<(), u16> {
        Err(EOPNOTSUPP)
    }
    fn listen(&mut self, _descriptor: u64, _backlog: u16) -> Result<(), u16> {
        Err(EOPNOTSUPP)
    }
    fn accept(&mut self, _descriptor: u64, _flags: u64) -> Result<(u64, [u8; 4], u16), u16> {
        Err(EOPNOTSUPP)
    }
    fn socket_name(&mut self, _descriptor: u64) -> Result<([u8; 4], u16), u16> {
        Err(EOPNOTSUPP)
    }
    fn seek(&mut self, _descriptor: u64, _offset: i64, _whence: u64) -> Result<u64, u16> {
        Err(EBADF)
    }
    fn mmap(
        &mut self,
        _address: u64,
        _length: u64,
        _protection: u64,
        _flags: u64,
        _descriptor: u64,
        _offset: u64,
    ) -> Result<u64, u16> {
        Err(ENOMEM)
    }
    fn munmap(&mut self, _address: u64, _length: u64) -> Result<(), u16> {
        Err(EINVAL)
    }
    fn mprotect(&mut self, _address: u64, _length: u64, _protection: u64) -> Result<(), u16> {
        Err(EOPNOTSUPP)
    }
    fn madvise(&mut self, _address: u64, _length: u64, _advice: u64) -> Result<(), u16> {
        Err(EOPNOTSUPP)
    }
    fn fs_base(&self) -> u64 {
        0
    }
    fn set_fs_base(&mut self, _base: u64) -> Result<(), u16> {
        Err(ENOSYS)
    }
    fn brk(&mut self, current: u64, _requested: u64) -> u64 {
        current
    }
    fn clock_gettime(&mut self, _clock_id: u64) -> Result<Timespec, u16> {
        Err(EINVAL)
    }
    fn clock_getres(&mut self, _clock_id: u64) -> Result<Timespec, u16> {
        Err(EINVAL)
    }
}

/// Despacha os números da ABI Linux x86_64. Retornos de erro seguem `-errno`
/// codificado em RAX. A saída limita uma chamada a 64 KiB por fairness.
pub fn dispatch<I: LinuxSyscallIo>(
    number: u64,
    arguments: [u64; 6],
    context: ProcessContext,
    io: &mut I,
) -> SyscallOutcome {
    match number {
        SYS_EXECVE => execve_syscall(arguments[0], arguments[1], arguments[2], io),
        SYS_READ => read_syscall(arguments[0], arguments[1], arguments[2], None, io),
        SYS_PREAD64 => read_syscall(
            arguments[0],
            arguments[1],
            arguments[2],
            Some(arguments[3]),
            io,
        ),
        SYS_OPEN => open_syscall(AT_FDCWD, arguments[0], arguments[1], arguments[2], io),
        SYS_FSTAT => fstat_syscall(arguments[0], arguments[1], io),
        SYS_GETRANDOM => getrandom_syscall(arguments[0], arguments[1], arguments[2] as u32, io),
        SYS_POLL => poll_syscall(arguments[0], arguments[1], arguments[2] as i32, io),
        SYS_RT_SIGACTION => {
            sigaction_syscall(arguments[0], arguments[1], arguments[2], arguments[3], io)
        }
        SYS_ACCESS => access_syscall(arguments[0], arguments[1], io),
        SYS_NEWFSTATAT => {
            newfstatat_syscall(arguments[0], arguments[1], arguments[2], arguments[3], io)
        }
        SYS_STATX => statx_syscall(
            arguments[0],
            arguments[1],
            arguments[2] as u32,
            arguments[3] as u32,
            arguments[4],
            io,
        ),
        SYS_OPENAT => open_syscall(arguments[0], arguments[1], arguments[2], arguments[3], io),
        SYS_CLOSE => result(io.close(arguments[0]).map(|_| 0)),
        SYS_FSYNC => result(io.fsync(arguments[0]).map(|_| 0)),
        SYS_SOCKET => result(io.socket(arguments[0], arguments[1], arguments[2])),
        SYS_ACCEPT | SYS_ACCEPT4 => accept_syscall(
            arguments[0],
            arguments[1],
            arguments[2],
            if number == SYS_ACCEPT4 {
                arguments[3]
            } else {
                0
            },
            io,
        ),
        SYS_SENDTO | SYS_RECVFROM if !crate::dircache_socket::is_descriptor(arguments[0]) && io.socket_name(arguments[0]).is_err() => errno(ENOTSOCK),
        SYS_SENDTO if arguments[3] & !(0x4000 | 0x40) == 0 && arguments[4] == 0 => {
            write_syscall(arguments[0], arguments[1], arguments[2], io)
        }
        SYS_RECVFROM if arguments[3] & !0x40 == 0 && arguments[4] == 0 => {
            read_syscall(arguments[0], arguments[1], arguments[2], None, io)
        }
        SYS_SENDTO | SYS_RECVFROM => errno(EOPNOTSUPP),
        SYS_BIND | SYS_CONNECT if crate::dircache_socket::is_descriptor(arguments[0]) => {
            unix_address_syscall(arguments[0], arguments[1], arguments[2], number == SYS_BIND, io)
        }
        SYS_CONNECT => errno(EOPNOTSUPP),
        SYS_BIND => bind_syscall(arguments[0], arguments[1], arguments[2], io),
        SYS_GETSOCKNAME => getsockname_syscall(arguments[0], arguments[1], arguments[2], io),
        SYS_LISTEN => {
            if (arguments[1] as i64) <= 0 {
                errno(EINVAL)
            } else {
                result(
                    io.listen(arguments[0], arguments[1].min(128) as u16)
                        .map(|_| 0),
                )
            }
        }
        SYS_LSEEK => result(io.seek(arguments[0], arguments[1] as i64, arguments[2])),
        SYS_MMAP => result(io.mmap(
            arguments[0],
            arguments[1],
            arguments[2],
            arguments[3],
            arguments[4],
            arguments[5],
        )),
        SYS_MUNMAP => result(io.munmap(arguments[0], arguments[1]).map(|_| 0)),
        SYS_MPROTECT => result(
            io.mprotect(arguments[0], arguments[1], arguments[2])
                .map(|_| 0),
        ),
        SYS_MADVISE => result(
            io.madvise(arguments[0], arguments[1], arguments[2])
                .map(|_| 0),
        ),
        SYS_ARCH_PRCTL => match arguments[0] {
            ARCH_SET_FS if arguments[1] >= crate::elf::USER_TOP => errno(EPERM),
            ARCH_SET_FS => result(io.set_fs_base(arguments[1]).map(|_| 0)),
            ARCH_GET_FS => {
                let base = io.fs_base();
                result(
                    io.copy_to_user(arguments[1], &base.to_le_bytes())
                        .map(|_| 0),
                )
            }
            _ => errno(EINVAL),
        },
        SYS_BRK => SyscallOutcome::Return(io.brk(context.program_break, arguments[0])),
        SYS_GETPID => SyscallOutcome::Return(context.process_id),
        SYS_GETTID => SyscallOutcome::Return(context.thread_id),
        SYS_GETCWD => getcwd_syscall(arguments[0], arguments[1], io),
        SYS_MKDIR => mkdir_syscall(arguments[0], arguments[1], io),
        SYS_UMASK => umask_syscall(arguments[0], io),
        SYS_CHMOD => chmod_syscall(arguments[0], arguments[1], io),
        SYS_FCHMOD => fchmod_syscall(arguments[0], arguments[1], io),
        SYS_CHOWN | SYS_LCHOWN => chown_syscall(arguments[0], arguments[1], arguments[2], io),
        SYS_FCHOWN => fchown_syscall(arguments[0], arguments[1], arguments[2], io),
        SYS_FCHOWNAT => fchownat_syscall(
            arguments[0],
            arguments[1],
            arguments[2],
            arguments[3],
            arguments[4],
            io,
        ),
        SYS_FCHMODAT => {
            fchmodat_syscall(arguments[0], arguments[1], arguments[2], arguments[3], io)
        }
        SYS_UNLINK => unlink_syscall(arguments[0], io),
        SYS_RMDIR => rmdir_syscall(arguments[0], io),
        SYS_UNLINKAT => {
            if arguments[3] & !AT_REMOVEDIR != 0 {
                errno(EOPNOTSUPP)
            } else {
                let mut path = [0u8; PATH_BUFFER_SIZE];
                match read_rooted_path(arguments[1], &mut path, io) {
                    Ok((path, absolute)) if arguments[0] as i32 == AT_FDCWD as i32 || absolute => {
                        let operation = if arguments[3] & AT_REMOVEDIR != 0 {
                            io.rmdir(path)
                        } else {
                            io.unlink(path)
                        };
                        result(operation.map(|_| 0))
                    }
                    Ok(_) => errno(EBADF),
                    Err(error) => errno(error),
                }
            }
        }
        SYS_CLOCK_GETTIME => clock_syscall(arguments[0], arguments[1], false, io),
        SYS_CLOCK_GETRES => clock_syscall(arguments[0], arguments[1], true, io),
        SYS_UNAME => uname_syscall(arguments[0], io),
        SYS_GETUID | SYS_GETEUID => SyscallOutcome::Return(context.uid as u64),
        SYS_GETGID | SYS_GETEGID => SyscallOutcome::Return(context.gid as u64),
        SYS_GETGROUPS => getgroups_syscall(
            arguments[0],
            arguments[1],
            &context.supplementary_groups,
            context.group_count as usize,
            io,
        ),
        SYS_EXIT | SYS_EXIT_GROUP => SyscallOutcome::Exit {
            status: arguments[0] as u8,
        },
        SYS_WRITEV if crate::dircache_socket::is_descriptor(arguments[0]) => errno(EOPNOTSUPP),
        SYS_WRITEV => writev_syscall(arguments[0], arguments[1], arguments[2], io),
        SYS_WRITE => write_syscall(arguments[0], arguments[1], arguments[2], io),
        _ => errno(ENOSYS),
    }
}

fn clock_syscall<I: LinuxSyscallIo>(
    clock_id: u64,
    address: u64,
    resolution: bool,
    io: &mut I,
) -> SyscallOutcome {
    // Linux permits a null resolution pointer for clock_getres as a query-only
    // operation; clock_gettime always requires an output buffer.
    if resolution && address == 0 {
        return SyscallOutcome::Return(0);
    }
    let value = if resolution {
        io.clock_getres(clock_id)
    } else {
        io.clock_gettime(clock_id)
    };
    let value = match value {
        Ok(value) => value,
        Err(error) => return errno(error),
    };
    if !(0..1_000_000_000).contains(&value.nanoseconds) || value.seconds < 0 {
        return errno(EIO);
    }
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&value.seconds.to_le_bytes());
    bytes[8..].copy_from_slice(&value.nanoseconds.to_le_bytes());
    result(io.copy_to_user(address, &bytes).map(|_| 0))
}

fn getrandom_syscall<I: LinuxSyscallIo>(
    address: u64,
    length: u64,
    flags: u32,
    io: &mut I,
) -> SyscallOutcome {
    const GRND_NONBLOCK: u32 = 0x1;
    const GRND_RANDOM: u32 = 0x2;
    const GRND_INSECURE: u32 = 0x4;
    if flags & !(GRND_NONBLOCK | GRND_RANDOM | GRND_INSECURE) != 0
        || flags & (GRND_RANDOM | GRND_INSECURE) == (GRND_RANDOM | GRND_INSECURE)
    {
        return errno(EINVAL);
    }
    let limit = if flags & GRND_RANDOM != 0 {
        512
    } else {
        MAX_WRITE_BYTES
    };
    let length = length.min(limit as u64) as usize;
    if length == 0 {
        return SyscallOutcome::Return(0);
    }
    if let Err(error) = io.validate_user_write(address, length) {
        return errno(error);
    }
    let mut buffer = zeroize::Zeroizing::new([0u8; WRITE_CHUNK_SIZE]);
    let mut copied = 0;
    while copied < length {
        let chunk = &mut buffer[..(length - copied).min(WRITE_CHUNK_SIZE)];
        if let Err(error) = io.random_bytes(chunk) {
            return partial_or_error(copied, error);
        }
        let Some(destination) = address.checked_add(copied as u64) else {
            return partial_or_error(copied, EFAULT);
        };
        if let Err(error) = io.copy_to_user(destination, chunk) {
            return partial_or_error(copied, error);
        }
        copied += chunk.len();
    }
    SyscallOutcome::Return(copied as u64)
}

fn poll_syscall<I: LinuxSyscallIo>(
    address: u64,
    count: u64,
    timeout: i32,
    io: &mut I,
) -> SyscallOutcome {
    const MAX_POLL_FDS: usize = 256;
    if timeout != 0 {
        return errno(ENOSYS);
    }
    if count > MAX_POLL_FDS as u64 {
        return errno(EINVAL);
    }
    if count == 0 {
        return SyscallOutcome::Return(0);
    }
    let length = count as usize * 8;
    let mut entries = [0u8; MAX_POLL_FDS * 8];
    let entries = &mut entries[..length];
    if let Err(error) = io.copy_from_user(address, entries) {
        return errno(error);
    }
    let mut ready = 0;
    for entry in entries.as_chunks_mut::<8>().0 {
        let fd = i32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]);
        let events = u16::from_le_bytes([entry[4], entry[5]]);
        let returned = if fd < 0 {
            0
        } else {
            match io.poll_events(fd as u32, events) {
                Ok(events) => events,
                Err(EBADF) => POLLNVAL,
                Err(error) => return errno(error),
            }
        };
        ready += u64::from(returned != 0);
        entry[6..].copy_from_slice(&returned.to_le_bytes());
    }
    result(io.copy_to_user(address, entries).map(|_| ready))
}

fn sigaction_syscall<I: LinuxSyscallIo>(
    signal: u64,
    action: u64,
    previous: u64,
    mask_size: u64,
    io: &mut I,
) -> SyscallOutcome {
    if mask_size != 8 || !(1..=64).contains(&signal) || matches!(signal, 9 | 19) {
        return errno(EINVAL);
    }
    if signal != SIGPIPE {
        return errno(ENOSYS);
    }
    // Linux x86-64 kernel sigaction: handler, flags, restorer, signal mask.
    let mut bytes = [0u8; 32];
    let ignored = if action == 0 {
        None
    } else {
        if let Err(error) = io.copy_from_user(action, &mut bytes) {
            return errno(error);
        }
        match u64::from_le_bytes(bytes[..8].try_into().unwrap()) {
            0 => Some(false),
            1 => Some(true),
            _ => return errno(ENOSYS),
        }
    };
    if previous != 0 {
        bytes.fill(0);
        bytes[0] = u8::from(io.ignores_sigpipe());
        if let Err(error) = io.copy_to_user(previous, &bytes) {
            return errno(error);
        }
    }
    result(
        ignored
            .map_or(Ok(()), |value| io.set_sigpipe_ignored(value))
            .map(|_| 0),
    )
}

fn fstat_syscall<I: LinuxSyscallIo>(descriptor: u64, address: u64, io: &mut I) -> SyscallOutcome {
    let stat = match io.fstat(descriptor) {
        Ok(stat) => stat,
        Err(error) => return errno(error),
    };
    write_stat(address, stat, io)
}

fn newfstatat_syscall<I: LinuxSyscallIo>(
    directory: u64,
    path_address: u64,
    stat_address: u64,
    flags: u64,
    io: &mut I,
) -> SyscallOutcome {
    if flags & !AT_EMPTY_PATH != 0 {
        return errno(EOPNOTSUPP);
    }
    match stat_at(directory, path_address, flags & AT_EMPTY_PATH != 0, io) {
        Ok(stat) => write_stat(stat_address, stat, io),
        Err(error) => errno(error),
    }
}

fn stat_at<I: LinuxSyscallIo>(
    directory: u64,
    path_address: u64,
    allow_empty: bool,
    io: &mut I,
) -> Result<LinuxStat, u16> {
    let directory = directory as i32;
    if allow_empty {
        let mut first = [0];
        if path_address != 0 {
            io.copy_from_user(path_address, &mut first)?;
        }
        if first[0] == 0 {
            return if directory == AT_FDCWD as i32 {
                io.stat("/")
            } else {
                io.fstat(directory as u32 as u64)
            };
        }
    }
    let mut path = [0u8; PATH_BUFFER_SIZE];
    let (path, absolute) = read_rooted_path(path_address, &mut path, io)?;
    if directory != AT_FDCWD as i32 && !absolute {
        return Err(EBADF);
    }
    io.stat(path)
}

fn statx_syscall<I: LinuxSyscallIo>(
    directory: u64,
    path: u64,
    flags: u32,
    mask: u32,
    address: u64,
    io: &mut I,
) -> SyscallOutcome {
    const AT_STATX_SYNC_TYPE: u32 = 0x6000;
    const STATX_RESERVED: u32 = 0x8000_0000;
    // TYPE, MODE, NLINK, UID, GID, INO and SIZE. The current VFS does
    // not report real allocation counts or timestamps.
    const AVAILABLE: u32 = 0x31f;
    let allowed = (AT_EMPTY_PATH | AT_SYMLINK_NOFOLLOW) as u32 | AT_STATX_SYNC_TYPE;
    if flags & !allowed != 0
        || flags & AT_STATX_SYNC_TYPE == AT_STATX_SYNC_TYPE
        || mask & STATX_RESERVED != 0
    {
        return errno(EINVAL);
    }
    let stat = match stat_at(directory, path, flags & AT_EMPTY_PATH as u32 != 0, io) {
        Ok(stat) => stat,
        Err(error) => return errno(error),
    };
    let (Ok(size), Ok(block_size), Ok(links)) = (
        u64::try_from(stat.size),
        u32::try_from(stat.block_size),
        u32::try_from(stat.links),
    ) else {
        return errno(EOVERFLOW);
    };
    let mut bytes = [0u8; 256];
    bytes[..4].copy_from_slice(&AVAILABLE.to_le_bytes());
    bytes[4..8].copy_from_slice(&block_size.to_le_bytes());
    bytes[16..20].copy_from_slice(&links.to_le_bytes());
    bytes[20..24].copy_from_slice(&stat.uid.to_le_bytes());
    bytes[24..28].copy_from_slice(&stat.gid.to_le_bytes());
    bytes[28..30].copy_from_slice(&(stat.mode as u16).to_le_bytes());
    bytes[32..40].copy_from_slice(&stat.inode.to_le_bytes());
    bytes[40..48].copy_from_slice(&size.to_le_bytes());
    result(io.copy_to_user(address, &bytes).map(|_| 0))
}

fn access_syscall<I: LinuxSyscallIo>(path_address: u64, mode: u64, io: &mut I) -> SyscallOutcome {
    if mode & !0x7 != 0 {
        return errno(EINVAL);
    }
    let mut path = [0u8; PATH_BUFFER_SIZE];
    let (path, _) = match read_rooted_path(path_address, &mut path, io) {
        Ok(path) => path,
        Err(error) => return errno(error),
    };
    result(io.access(path, mode).map(|_| 0))
}

fn mkdir_syscall<I: LinuxSyscallIo>(path_address: u64, mode: u64, io: &mut I) -> SyscallOutcome {
    let mut path = [0u8; PATH_BUFFER_SIZE];
    let (path, _) = match read_rooted_path(path_address, &mut path, io) {
        Ok(path) => path,
        Err(error) => return errno(error),
    };
    let mode = match u16::try_from(mode) {
        Ok(mode) => mode,
        Err(_) => return errno(EINVAL),
    };
    result(io.mkdir(path, mode).map(|_| 0))
}

fn umask_syscall<I: LinuxSyscallIo>(mask: u64, io: &mut I) -> SyscallOutcome {
    let mask = match u16::try_from(mask) {
        Ok(mask) => mask & 0o777,
        Err(_) => return errno(EINVAL),
    };
    result(io.umask(mask).map(u64::from))
}

fn chmod_syscall<I: LinuxSyscallIo>(path_address: u64, mode: u64, io: &mut I) -> SyscallOutcome {
    let mut path = [0u8; PATH_BUFFER_SIZE];
    let (path, _) = match read_rooted_path(path_address, &mut path, io) {
        Ok(path) => path,
        Err(error) => return errno(error),
    };
    let mode = match u16::try_from(mode) {
        Ok(mode) => mode,
        Err(_) => return errno(EINVAL),
    };
    result(io.chmod(path, mode).map(|_| 0))
}

fn fchmod_syscall<I: LinuxSyscallIo>(descriptor: u64, mode: u64, io: &mut I) -> SyscallOutcome {
    let mode = match u16::try_from(mode) {
        Ok(mode) => mode,
        Err(_) => return errno(EINVAL),
    };
    result(io.fchmod(descriptor, mode).map(|_| 0))
}

fn chown_syscall<I: LinuxSyscallIo>(
    path_address: u64,
    uid: u64,
    gid: u64,
    io: &mut I,
) -> SyscallOutcome {
    let uid = match decode_chown_id(uid) {
        Ok(uid) => uid,
        Err(error) => return errno(error),
    };
    let gid = match decode_chown_id(gid) {
        Ok(gid) => gid,
        Err(error) => return errno(error),
    };
    let mut path = [0u8; PATH_BUFFER_SIZE];
    let (path, _) = match read_rooted_path(path_address, &mut path, io) {
        Ok(path) => path,
        Err(error) => return errno(error),
    };
    result(io.chown(path, uid, gid).map(|_| 0))
}

fn fchown_syscall<I: LinuxSyscallIo>(
    descriptor: u64,
    uid: u64,
    gid: u64,
    io: &mut I,
) -> SyscallOutcome {
    let uid = match decode_chown_id(uid) {
        Ok(uid) => uid,
        Err(error) => return errno(error),
    };
    let gid = match decode_chown_id(gid) {
        Ok(gid) => gid,
        Err(error) => return errno(error),
    };
    result(io.fchown(descriptor, uid, gid).map(|_| 0))
}

fn fchownat_syscall<I: LinuxSyscallIo>(
    directory: u64,
    path_address: u64,
    uid: u64,
    gid: u64,
    flags: u64,
    io: &mut I,
) -> SyscallOutcome {
    if flags & !AT_SYMLINK_NOFOLLOW != 0 {
        return errno(EOPNOTSUPP);
    }
    let uid = match decode_chown_id(uid) {
        Ok(uid) => uid,
        Err(error) => return errno(error),
    };
    let gid = match decode_chown_id(gid) {
        Ok(gid) => gid,
        Err(error) => return errno(error),
    };
    let mut path = [0u8; PATH_BUFFER_SIZE];
    match read_rooted_path(path_address, &mut path, io) {
        Ok((path, absolute)) if directory as i32 == AT_FDCWD as i32 || absolute => {
            result(io.chown(path, uid, gid).map(|_| 0))
        }
        Ok(_) => errno(EBADF),
        Err(error) => errno(error),
    }
}

fn fchmodat_syscall<I: LinuxSyscallIo>(
    directory: u64,
    path_address: u64,
    mode: u64,
    flags: u64,
    io: &mut I,
) -> SyscallOutcome {
    if flags & !AT_SYMLINK_NOFOLLOW != 0 {
        return errno(EOPNOTSUPP);
    }
    let mode = match u16::try_from(mode) {
        Ok(mode) => mode,
        Err(_) => return errno(EINVAL),
    };
    let mut path = [0u8; PATH_BUFFER_SIZE];
    match read_rooted_path(path_address, &mut path, io) {
        Ok((path, absolute)) if directory as i32 == AT_FDCWD as i32 || absolute => {
            result(io.chmod(path, mode).map(|_| 0))
        }
        Ok(_) => errno(EBADF),
        Err(error) => errno(error),
    }
}

fn decode_chown_id(value: u64) -> Result<Option<u32>, u16> {
    let id = u32::try_from(value).map_err(|_| EINVAL)?;
    Ok((id != u32::MAX).then_some(id))
}

fn unlink_syscall<I: LinuxSyscallIo>(path_address: u64, io: &mut I) -> SyscallOutcome {
    let mut path = [0u8; PATH_BUFFER_SIZE];
    let (path, _) = match read_rooted_path(path_address, &mut path, io) {
        Ok(path) => path,
        Err(error) => return errno(error),
    };
    result(io.unlink(path).map(|_| 0))
}

fn rmdir_syscall<I: LinuxSyscallIo>(path_address: u64, io: &mut I) -> SyscallOutcome {
    let mut path = [0u8; PATH_BUFFER_SIZE];
    let (path, _) = match read_rooted_path(path_address, &mut path, io) {
        Ok(path) => path,
        Err(error) => return errno(error),
    };
    result(io.rmdir(path).map(|_| 0))
}

fn getcwd_syscall<I: LinuxSyscallIo>(address: u64, size: u64, io: &mut I) -> SyscallOutcome {
    const ROOT_CWD: [u8; 2] = [b'/', 0];
    if size < ROOT_CWD.len() as u64 {
        return errno(ERANGE);
    }
    if io.copy_to_user(address, &ROOT_CWD).is_err() {
        return errno(EFAULT);
    }
    SyscallOutcome::Return(ROOT_CWD.len() as u64)
}

fn write_stat<I: LinuxSyscallIo>(address: u64, stat: LinuxStat, io: &mut I) -> SyscallOutcome {
    const STAT_SIZE: usize = 144;
    if address == 0 || !address.is_multiple_of(8) {
        return errno(EFAULT);
    }
    if stat.size < 0 || stat.block_size < 0 || stat.blocks < 0 {
        return errno(EIO);
    }
    let mut bytes = [0u8; STAT_SIZE];
    bytes[8..16].copy_from_slice(&stat.inode.to_le_bytes());
    bytes[16..24].copy_from_slice(&stat.links.to_le_bytes());
    bytes[24..28].copy_from_slice(&stat.mode.to_le_bytes());
    bytes[28..32].copy_from_slice(&stat.uid.to_le_bytes());
    bytes[32..36].copy_from_slice(&stat.gid.to_le_bytes());
    bytes[48..56].copy_from_slice(&stat.size.to_le_bytes());
    bytes[56..64].copy_from_slice(&stat.block_size.to_le_bytes());
    bytes[64..72].copy_from_slice(&stat.blocks.to_le_bytes());
    for (offset, value) in [
        (72, stat.access_time),
        (88, stat.modification_time),
        (104, stat.change_time),
    ] {
        bytes[offset..offset + 8].copy_from_slice(&value.seconds.to_le_bytes());
        bytes[offset + 8..offset + 16].copy_from_slice(&value.nanoseconds.to_le_bytes());
    }
    result(io.copy_to_user(address, &bytes).map(|_| 0))
}

/// Preenche o `struct utsname` Linux x86_64 (seis campos de 65 bytes).
/// Strings estáveis e terminadas em NUL permitem que libc e ferramentas
/// identifiquem a ABI sem expor dados do host.
fn uname_syscall<I: LinuxSyscallIo>(address: u64, io: &mut I) -> SyscallOutcome {
    const FIELD: usize = 65;
    let mut value = [0u8; FIELD * 6];
    for (index, field) in [
        "Linux",
        "carlio",
        "0.1.0-carlio",
        "#1 CarlioOS",
        "x86_64",
        "(none)",
    ]
    .into_iter()
    .enumerate()
    {
        let offset = index * FIELD;
        value[offset..offset + field.len()].copy_from_slice(field.as_bytes());
    }
    result(io.copy_to_user(address, &value).map(|_| 0))
}

fn getsockname_syscall<I: LinuxSyscallIo>(
    descriptor: u64,
    address: u64,
    length_ptr: u64,
    io: &mut I,
) -> SyscallOutcome {
    if crate::dircache_socket::is_descriptor(descriptor) {
        return result(io.unix_name(descriptor)
            .and_then(|bound| write_unix_name(address, length_ptr, bound, io))
            .map(|()| 0));
    }
    let (ip, port) = match io.socket_name(descriptor) {
        Ok(endpoint) => endpoint,
        Err(error) => return errno(error),
    };
    result(write_sockaddr(address, length_ptr, ip, port, io).map(|()| 0))
}

fn accept_syscall<I: LinuxSyscallIo>(
    descriptor: u64,
    address: u64,
    length_ptr: u64,
    flags: u64,
    io: &mut I,
) -> SyscallOutcome {
    if flags & !(0x800 | 0x80000) != 0 {
        return errno(EINVAL);
    }
    let (accepted, ip, port) = match io.accept(descriptor, flags) {
        Ok(endpoint) => endpoint,
        Err(error) => return errno(error),
    };
    if address != 0 {
        let written = if crate::dircache_socket::is_descriptor(accepted) {
            write_unix_name(address, length_ptr, false, io)
        } else {
            write_sockaddr(address, length_ptr, ip, port, io)
        };
        if let Err(error) = written {
            let _ = io.close(accepted);
            return errno(error);
        }
    }
    SyscallOutcome::Return(accepted)
}

fn write_sockaddr<I: LinuxSyscallIo>(
    address: u64,
    length_ptr: u64,
    ip: [u8; 4],
    port: u16,
    io: &mut I,
) -> Result<(), u16> {
    let mut length = [0; 4];
    io.copy_from_user(length_ptr, &mut length)?;
    let requested = i32::from_le_bytes(length);
    if requested < 0 {
        return Err(EINVAL);
    }
    let mut sockaddr = [0u8; 16];
    sockaddr[..2].copy_from_slice(&2u16.to_le_bytes());
    sockaddr[2..4].copy_from_slice(&port.to_be_bytes());
    sockaddr[4..8].copy_from_slice(&ip);
    let amount = (requested as usize).min(sockaddr.len());
    if amount != 0 {
        io.copy_to_user(address, &sockaddr[..amount])?;
    }
    io.copy_to_user(length_ptr, &16u32.to_le_bytes())
}

fn write_unix_name<I: LinuxSyscallIo>(
    address: u64,
    length_ptr: u64,
    bound: bool,
    io: &mut I,
) -> Result<(), u16> {
    let mut length = [0; 4];
    io.copy_from_user(length_ptr, &mut length)?;
    let requested = i32::from_le_bytes(length);
    if requested < 0 {
        return Err(EINVAL);
    }
    let mut sockaddr = [0u8; 110];
    sockaddr[0] = 1;
    let actual = if bound {
        let path = crate::dircache::SOCKET_PATH.as_bytes();
        sockaddr[2..2 + path.len()].copy_from_slice(path);
        3 + path.len()
    } else {
        2
    };
    let amount = (requested as usize).min(actual);
    if amount != 0 {
        io.copy_to_user(address, &sockaddr[..amount])?;
    }
    io.copy_to_user(length_ptr, &(actual as u32).to_le_bytes())
}

fn unix_address_syscall<I: LinuxSyscallIo>(
    fd: u64,
    address: u64,
    length: u64,
    bind: bool,
    io: &mut I,
) -> SyscallOutcome {
    if !(3..=110).contains(&length) {
        return errno(EINVAL);
    }
    let mut storage = [0; 110];
    let sockaddr = &mut storage[..length as usize];
    if let Err(error) = io.copy_from_user(address, sockaddr) {
        return errno(error);
    }
    if sockaddr[..2] != [1, 0] {
        return errno(EAFNOSUPPORT);
    }
    let path = &sockaddr[2..];
    let end = path.iter().position(|byte| *byte == 0).unwrap_or(path.len());
    result(io.unix_address(fd, &path[..end], bind).map(|()| 0))
}

fn bind_syscall<I: LinuxSyscallIo>(
    descriptor: u64,
    address: u64,
    length: u64,
    io: &mut I,
) -> SyscallOutcome {
    if length < 16 {
        return errno(EINVAL);
    }
    let mut sockaddr = [0u8; 16];
    if io.copy_from_user(address, &mut sockaddr).is_err() {
        return errno(EFAULT);
    }
    if u16::from_le_bytes([sockaddr[0], sockaddr[1]]) != 2 {
        return errno(EAFNOSUPPORT);
    }
    let port = u16::from_be_bytes([sockaddr[2], sockaddr[3]]);
    result(
        io.bind(
            descriptor,
            sockaddr[4..8].try_into().unwrap_or([0; 4]),
            port,
        )
        .map(|_| 0),
    )
}

fn open_syscall<I: LinuxSyscallIo>(
    dirfd: u64,
    address: u64,
    flags: u64,
    mode: u64,
    io: &mut I,
) -> SyscallOutcome {
    let allowed = O_ACCMODE | O_CREAT | O_EXCL | O_TRUNC | O_APPEND | O_CLOEXEC;
    if flags & !allowed != 0 || flags & O_ACCMODE == 3 {
        return errno(EOPNOTSUPP);
    }
    if flags & O_CREAT != 0 && u16::try_from(mode).is_err() {
        return errno(EINVAL);
    }
    let writable = flags & O_ACCMODE == O_WRONLY || flags & O_ACCMODE == O_RDWR;
    if flags & O_TRUNC != 0 && !writable || flags & O_APPEND != 0 && !writable {
        return errno(EINVAL);
    }
    let mut bytes = [0u8; PATH_BUFFER_SIZE];
    let (path, absolute) = match read_rooted_path(address, &mut bytes, io) {
        Ok(path) => path,
        Err(error) => return errno(error),
    };
    if dirfd as i32 != AT_FDCWD as i32 && !absolute {
        return errno(EBADF);
    }
    result(io.open(path, flags, mode))
}

fn execve_syscall<I: LinuxSyscallIo>(
    path_address: u64,
    arguments_address: u64,
    environment_address: u64,
    io: &mut I,
) -> SyscallOutcome {
    let mut path_bytes = [0u8; PATH_BUFFER_SIZE];
    let (path, _) = match read_rooted_path(path_address, &mut path_bytes, io) {
        Ok(value) => value,
        Err(error) => return errno(error),
    };
    let mut argument_bytes = [[0u8; EXEC_STRING_LIMIT]; EXEC_VECTOR_LIMIT];
    let mut environment_bytes = [[0u8; EXEC_STRING_LIMIT]; EXEC_VECTOR_LIMIT];
    let mut argument_lengths = [0usize; EXEC_VECTOR_LIMIT];
    let mut environment_lengths = [0usize; EXEC_VECTOR_LIMIT];
    let argument_count = match read_exec_vector(
        arguments_address,
        &mut argument_bytes,
        &mut argument_lengths,
        io,
    ) {
        Ok(count) => count,
        Err(error) => return errno(error),
    };
    let environment_count = match read_exec_vector(
        environment_address,
        &mut environment_bytes,
        &mut environment_lengths,
        io,
    ) {
        Ok(count) => count,
        Err(error) => return errno(error),
    };
    let mut arguments = [""; EXEC_VECTOR_LIMIT];
    for index in 0..argument_count {
        arguments[index] =
            match core::str::from_utf8(&argument_bytes[index][..argument_lengths[index]]) {
                Ok(value) => value,
                Err(_) => return errno(EINVAL),
            };
    }
    let mut environment = [""; EXEC_VECTOR_LIMIT];
    for index in 0..environment_count {
        environment[index] =
            match core::str::from_utf8(&environment_bytes[index][..environment_lengths[index]]) {
                Ok(value) => value,
                Err(_) => return errno(EINVAL),
            };
    }
    match io.execve(
        path,
        // Linux inserts an empty argv[0] when execve receives no arguments.
        &arguments[..argument_count.max(1)],
        &environment[..environment_count],
    ) {
        Ok(transition) => SyscallOutcome::Exec(transition),
        Err(error) => errno(error),
    }
}

fn read_exec_vector<I: LinuxSyscallIo>(
    vector_address: u64,
    strings: &mut [[u8; EXEC_STRING_LIMIT]; EXEC_VECTOR_LIMIT],
    lengths: &mut [usize; EXEC_VECTOR_LIMIT],
    io: &mut I,
) -> Result<usize, u16> {
    if vector_address == 0 {
        return Ok(0);
    }
    for index in 0..EXEC_VECTOR_LIMIT {
        let slot = vector_address
            .checked_add((index * core::mem::size_of::<u64>()) as u64)
            .ok_or(EFAULT)?;
        let mut pointer_bytes = [0u8; 8];
        io.copy_from_user(slot, &mut pointer_bytes)
            .map_err(|_| EFAULT)?;
        let pointer = u64::from_le_bytes(pointer_bytes);
        if pointer == 0 {
            return Ok(index);
        }
        lengths[index] = read_exec_string(pointer, &mut strings[index], io)?;
    }
    Err(E2BIG)
}

fn read_exec_string<I: LinuxSyscallIo>(
    address: u64,
    output: &mut [u8; EXEC_STRING_LIMIT],
    io: &mut I,
) -> Result<usize, u16> {
    for index in 0..output.len() {
        let location = address.checked_add(index as u64).ok_or(EFAULT)?;
        io.copy_from_user(location, &mut output[index..index + 1])
            .map_err(|_| EFAULT)?;
        if output[index] == 0 {
            return Ok(index);
        }
    }
    Err(E2BIG)
}

/// Lê uma string C limitada e interpreta caminhos relativos sob cwd `/`, que
/// é o único diretório de trabalho presente no modelo atual de processo.
fn read_rooted_path<'a, I: LinuxSyscallIo>(
    address: u64,
    bytes: &'a mut [u8; PATH_BUFFER_SIZE],
    io: &mut I,
) -> Result<(&'a str, bool), u16> {
    let mut length = 0usize;
    loop {
        if length == bytes.len() {
            return Err(EINVAL);
        }
        let location = address.checked_add(length as u64).ok_or(EFAULT)?;
        io.copy_from_user(location, &mut bytes[length..length + 1])
            .map_err(|_| EFAULT)?;
        if bytes[length] == 0 {
            break;
        }
        length += 1;
    }
    if length == 0 {
        return Err(EINVAL);
    }
    let absolute = bytes[0] == b'/';
    if !absolute {
        if length + 1 >= bytes.len() {
            return Err(EINVAL);
        }
        bytes.copy_within(0..length, 1);
        bytes[0] = b'/';
        length += 1;
        bytes[length] = 0;
    }
    let path = core::str::from_utf8(&bytes[..length]).map_err(|_| EINVAL)?;
    Ok((path, absolute))
}

fn result(value: Result<u64, u16>) -> SyscallOutcome {
    match value {
        Ok(value) => SyscallOutcome::Return(value),
        Err(error) => errno(error),
    }
}

fn read_syscall<I: LinuxSyscallIo>(
    descriptor: u64,
    address: u64,
    requested: u64,
    offset: Option<u64>,
    io: &mut I,
) -> SyscallOutcome {
    if crate::dircache_socket::is_descriptor(descriptor) {
        if offset.is_some() {
            return errno(ESPIPE);
        }
        let mut packet = [0; crate::dircache_socket::PACKET_SIZE];
        let count = requested.min(packet.len() as u64) as usize;
        if count == 0 {
            return SyscallOutcome::Return(0);
        }
        return result(io.validate_user_write(address, count).and_then(|()| {
            let amount = io.read(descriptor, &mut packet[..count])?;
            io.copy_to_user(address, packet.get(..amount).ok_or(EIO)?)?;
            Ok(amount as u64)
        }));
    }
    if offset.is_some_and(|offset| offset > i64::MAX as u64) {
        return errno(EINVAL);
    }
    let count = requested.min(MAX_WRITE_BYTES as u64) as usize;
    if count == 0 {
        return SyscallOutcome::Return(0);
    }
    if let Err(error) = io.validate_user_write(address, count) {
        return errno(if error == 0 { EFAULT } else { error });
    }
    let mut buffer = [0u8; WRITE_CHUNK_SIZE];
    let mut read = 0usize;
    while read < count {
        let chunk = (count - read).min(buffer.len());
        let operation = match offset {
            Some(offset) => {
                let position = offset + read as u64;
                if position > i64::MAX as u64 {
                    return partial_or_error(read, EOVERFLOW);
                }
                io.pread(descriptor, position, &mut buffer[..chunk])
            }
            None => io.read(descriptor, &mut buffer[..chunk]),
        };
        let amount = match operation {
            Ok(0) => break,
            Ok(amount) if amount <= chunk => amount,
            Ok(_) => return partial_or_error(read, EIO),
            Err(error) => return partial_or_error(read, error),
        };
        let user_address = match address.checked_add(read as u64) {
            Some(value) => value,
            None => return partial_or_error(read, EFAULT),
        };
        if let Err(error) = io.copy_to_user(user_address, &buffer[..amount]) {
            return partial_or_error(read, if error == 0 { EFAULT } else { error });
        }
        read += amount;
        if amount < chunk {
            break;
        }
    }
    SyscallOutcome::Return(read as u64)
}

fn getgroups_syscall<I: LinuxSyscallIo>(
    size: u64,
    address: u64,
    groups: &[u32; 8],
    group_count: usize,
    io: &mut I,
) -> SyscallOutcome {
    if group_count > groups.len() || size > i32::MAX as u64 {
        return errno(EINVAL);
    }
    if size == 0 {
        return SyscallOutcome::Return(group_count as u64);
    }
    if size < group_count as u64 {
        return errno(EINVAL);
    }
    let mut bytes = [0u8; 8 * core::mem::size_of::<u32>()];
    for (index, gid) in groups.iter().take(group_count).enumerate() {
        let offset = index * core::mem::size_of::<u32>();
        bytes[offset..offset + 4].copy_from_slice(&gid.to_le_bytes());
    }
    let length = group_count * core::mem::size_of::<u32>();
    if io.copy_to_user(address, &bytes[..length]).is_err() {
        return errno(EFAULT);
    }
    SyscallOutcome::Return(group_count as u64)
}

fn writev_syscall<I: LinuxSyscallIo>(
    descriptor: u64,
    address: u64,
    vector_count: u64,
    io: &mut I,
) -> SyscallOutcome {
    if vector_count > 1024 {
        return errno(EINVAL);
    }
    let mut written = 0;
    for index in 0..vector_count {
        let Some(vector_address) = address.checked_add(index * 16) else {
            return partial_or_error(written, EFAULT);
        };
        let mut vector = [0; 16];
        if io.copy_from_user(vector_address, &mut vector).is_err() {
            return partial_or_error(written, EFAULT);
        }
        let base = u64::from_le_bytes(vector[..8].try_into().unwrap());
        let length = u64::from_le_bytes(vector[8..].try_into().unwrap());
        let count = length.min((MAX_WRITE_BYTES - written) as u64);
        let SyscallOutcome::Return(value) = write_syscall(descriptor, base, count, io) else {
            return partial_or_error(written, EIO);
        };
        if (value as i64) < 0 {
            return partial_or_error(written, (-(value as i64)) as u16);
        }
        written += value as usize;
        if value < length || written == MAX_WRITE_BYTES {
            break;
        }
    }
    SyscallOutcome::Return(written as u64)
}

fn write_syscall<I: LinuxSyscallIo>(
    descriptor: u64,
    address: u64,
    requested: u64,
    io: &mut I,
) -> SyscallOutcome {
    if requested == 0 {
        return SyscallOutcome::Return(0);
    }
    if crate::dircache_socket::is_descriptor(descriptor) {
        let mut packet = [0; crate::dircache_socket::PACKET_SIZE];
        if requested > packet.len() as u64 {
            return errno(EMSGSIZE);
        }
        let packet = &mut packet[..requested as usize];
        return result(io.copy_from_user(address, packet)
            .and_then(|()| io.write(descriptor, packet))
            .map(|amount| amount as u64));
    }
    let count = requested.min(MAX_WRITE_BYTES as u64) as usize;
    let mut buffer = [0u8; WRITE_CHUNK_SIZE];
    let mut written = 0usize;
    while written < count {
        let chunk = (count - written).min(buffer.len());
        let user_address = match address.checked_add(written as u64) {
            Some(value) => value,
            None => return partial_or_error(written, EFAULT),
        };
        if let Err(error) = io.copy_from_user(user_address, &mut buffer[..chunk]) {
            return partial_or_error(written, if error == 0 { EFAULT } else { error });
        }
        match io.write(descriptor, &buffer[..chunk]) {
            Ok(0) => break,
            Ok(amount) if amount <= chunk => {
                written += amount;
                if amount != chunk {
                    break;
                }
            }
            Ok(_) => return partial_or_error(written, EIO),
            Err(error) => return partial_or_error(written, error),
        }
    }
    SyscallOutcome::Return(written as u64)
}

fn partial_or_error(written: usize, error: u16) -> SyscallOutcome {
    if written > 0 {
        SyscallOutcome::Return(written as u64)
    } else {
        errno(error)
    }
}

const fn errno(error: u16) -> SyscallOutcome {
    SyscallOutcome::Return((-(error as i64)) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::string::String;
    use std::vec::Vec;

    struct FakeIo {
        base: u64,
        user: Vec<u8>,
        output: Vec<u8>,
        input: Vec<u8>,
        input_offset: usize,
        descriptor: u64,
        max_write: usize,
        opened_path: String,
        ignores_sigpipe: bool,
    }

    impl LinuxSyscallIo for FakeIo {
        fn copy_from_user(&mut self, address: u64, destination: &mut [u8]) -> Result<(), u16> {
            let offset = address.checked_sub(self.base).ok_or(EFAULT)? as usize;
            let end = offset.checked_add(destination.len()).ok_or(EFAULT)?;
            let source = self.user.get(offset..end).ok_or(EFAULT)?;
            destination.copy_from_slice(source);
            Ok(())
        }

        fn copy_to_user(&mut self, address: u64, source: &[u8]) -> Result<(), u16> {
            let offset = address.checked_sub(self.base).ok_or(EFAULT)? as usize;
            let end = offset.checked_add(source.len()).ok_or(EFAULT)?;
            let destination = self.user.get_mut(offset..end).ok_or(EFAULT)?;
            destination.copy_from_slice(source);
            Ok(())
        }

        fn validate_user_write(&mut self, address: u64, length: usize) -> Result<(), u16> {
            let offset = address.checked_sub(self.base).ok_or(EFAULT)? as usize;
            let end = offset.checked_add(length).ok_or(EFAULT)?;
            if end <= self.user.len() {
                Ok(())
            } else {
                Err(EFAULT)
            }
        }

        fn read(&mut self, descriptor: u64, destination: &mut [u8]) -> Result<usize, u16> {
            if descriptor != self.descriptor {
                return Err(EBADF);
            }
            let count = destination
                .len()
                .min(self.input.len().saturating_sub(self.input_offset));
            destination[..count]
                .copy_from_slice(&self.input[self.input_offset..self.input_offset + count]);
            self.input_offset += count;
            Ok(count)
        }

        fn fstat(&mut self, descriptor: u64) -> Result<LinuxStat, u16> {
            if descriptor != self.descriptor {
                return Err(EBADF);
            }
            let timestamp = Timespec {
                seconds: 0,
                nanoseconds: 0,
            };
            Ok(LinuxStat {
                inode: 3,
                mode: 0o020600,
                links: 1,
                uid: 1000,
                gid: 100,
                size: 0,
                block_size: 4096,
                blocks: 0,
                access_time: timestamp,
                modification_time: timestamp,
                change_time: timestamp,
            })
        }

        fn random_bytes(&mut self, destination: &mut [u8]) -> Result<(), u16> {
            destination.fill(0xa5);
            Ok(())
        }

        fn poll_events(&mut self, descriptor: u32, events: u16) -> Result<u16, u16> {
            if u64::from(descriptor) != self.descriptor {
                return Err(EBADF);
            }
            Ok(events & POLLOUT)
        }

        fn ignores_sigpipe(&self) -> bool {
            self.ignores_sigpipe
        }

        fn set_sigpipe_ignored(&mut self, ignored: bool) -> Result<(), u16> {
            self.ignores_sigpipe = ignored;
            Ok(())
        }

        fn open(&mut self, path: &str, _flags: u64, _mode: u64) -> Result<u64, u16> {
            self.opened_path.clear();
            self.opened_path.push_str(path);
            Ok(42)
        }

        fn write(&mut self, descriptor: u64, bytes: &[u8]) -> Result<usize, u16> {
            if descriptor != self.descriptor {
                return Err(EBADF);
            }
            let count = bytes.len().min(self.max_write);
            self.output.extend_from_slice(&bytes[..count]);
            Ok(count)
        }

        fn socket(&mut self, domain: u64, kind: u64, protocol: u64) -> Result<u64, u16> {
            if domain == 2 && matches!((kind, protocol), (1, 0 | 6) | (2, 0 | 17)) {
                Ok(5)
            } else {
                Err(EOPNOTSUPP)
            }
        }

        fn bind(&mut self, descriptor: u64, _address: [u8; 4], port: u16) -> Result<(), u16> {
            if descriptor == 5 && port != 0 {
                Ok(())
            } else {
                Err(EINVAL)
            }
        }

        fn listen(&mut self, descriptor: u64, backlog: u16) -> Result<(), u16> {
            if descriptor == 5 && backlog != 0 {
                Ok(())
            } else {
                Err(EINVAL)
            }
        }

        fn socket_name(&mut self, descriptor: u64) -> Result<([u8; 4], u16), u16> {
            if descriptor != 5 {
                return Err(EBADF);
            }
            Ok(([127, 0, 0, 1], 49152))
        }

        fn clock_gettime(&mut self, clock_id: u64) -> Result<Timespec, u16> {
            if clock_id == CLOCK_MONOTONIC {
                Ok(Timespec {
                    seconds: 12,
                    nanoseconds: 340_000_000,
                })
            } else {
                Err(EINVAL)
            }
        }

        fn clock_getres(&mut self, clock_id: u64) -> Result<Timespec, u16> {
            if clock_id == CLOCK_MONOTONIC {
                Ok(Timespec {
                    seconds: 0,
                    nanoseconds: 10_000_000,
                })
            } else {
                Err(EINVAL)
            }
        }
    }

    fn fake_io() -> FakeIo {
        let mut user = vec![0; 4096];
        user[..b"ferramenta linux\n".len()].copy_from_slice(b"ferramenta linux\n");
        FakeIo {
            base: 0x4000,
            user,
            output: Vec::new(),
            input: b"entrada linux\n".to_vec(),
            input_offset: 0,
            descriptor: 1,
            max_write: usize::MAX,
            opened_path: String::new(),
            ignores_sigpipe: false,
        }
    }

    #[test]
    fn statx_empty_path_uses_descriptor_and_marks_unavailable_fields() {
        let mut io = fake_io();
        let base = io.base;
        io.user[..256].fill(0xff);
        assert_eq!(
            statx_syscall(1, 0, AT_EMPTY_PATH as u32, 0xfff, base, &mut io),
            SyscallOutcome::Return(0)
        );
        assert_eq!(&io.user[..4], &0x31fu32.to_le_bytes());
        assert_eq!(&io.user[28..30], &0o020600u16.to_le_bytes());
        assert_eq!(&io.user[32..40], &3u64.to_le_bytes());
        assert_eq!(&io.user[64..128], &[0; 64]);
        assert_eq!(
            statx_syscall(3, 0, AT_EMPTY_PATH as u32, 0, base, &mut io),
            errno(EBADF)
        );
        assert_eq!(
            statx_syscall(1, 0, AT_EMPTY_PATH as u32, 1 << 31, base, &mut io),
            errno(EINVAL)
        );
    }

    #[test]
    fn immediate_poll_reports_ready_invalid_and_ignored_descriptors() {
        let mut io = fake_io();
        let base = io.base;
        for (entry, fd) in io.user[..24]
            .as_chunks_mut::<8>()
            .0
            .iter_mut()
            .zip([1i32, 4, -1])
        {
            entry[..4].copy_from_slice(&fd.to_le_bytes());
            entry[4..6].copy_from_slice(&POLLOUT.to_le_bytes());
            entry[6..].fill(0xff);
        }
        assert_eq!(poll_syscall(base, 3, 0, &mut io), SyscallOutcome::Return(2));
        assert_eq!(&io.user[6..8], &POLLOUT.to_le_bytes());
        assert_eq!(&io.user[14..16], &POLLNVAL.to_le_bytes());
        assert_eq!(&io.user[22..24], &[0, 0]);
        assert_eq!(poll_syscall(base, 3, 1, &mut io), errno(ENOSYS));
        assert_eq!(poll_syscall(0, 1, 0, &mut io), errno(EFAULT));
    }

    #[test]
    fn sigpipe_disposition_survives_query_and_bad_output_does_not_change_it() {
        let mut io = fake_io();
        let base = io.base;
        io.user[..32].fill(0);
        io.user[0] = 1;
        assert_eq!(
            sigaction_syscall(SIGPIPE, base, base + 32, 8, &mut io),
            SyscallOutcome::Return(0)
        );
        assert_eq!(io.user[32], 0);
        assert_eq!(
            sigaction_syscall(SIGPIPE, 0, base + 32, 8, &mut io),
            SyscallOutcome::Return(0)
        );
        assert_eq!(io.user[32], 1);
        io.user[0] = 0;
        assert_eq!(
            sigaction_syscall(SIGPIPE, base, 1, 8, &mut io),
            errno(EFAULT)
        );
        assert!(io.ignores_sigpipe());
    }

    #[test]
    fn getrandom_checks_flags_and_destination_before_copying() {
        let mut io = fake_io();
        let base = io.base;
        assert_eq!(
            getrandom_syscall(base, 300, 0, &mut io),
            SyscallOutcome::Return(300)
        );
        assert_eq!(&io.user[..300], &[0xa5; 300]);
        assert_eq!(getrandom_syscall(base, 1, 6, &mut io), errno(EINVAL));
        assert_eq!(getrandom_syscall(base + 4095, 2, 0, &mut io), errno(EFAULT));
    }

    #[test]
    fn dispatches_linux_getpid_exit_and_exit_group() {
        let mut io = fake_io();
        let context = ProcessContext {
            process_id: 40,
            thread_id: 42,
            uid: 1000,
            gid: 2000,
            supplementary_groups: [3000, 4000, 0, 0, 0, 0, 0, 0],
            group_count: 2,
            program_break: 0,
        };
        assert_eq!(
            dispatch(SYS_GETPID, [0; 6], context, &mut io),
            SyscallOutcome::Return(40)
        );
        assert_eq!(
            dispatch(SYS_GETTID, [0; 6], context, &mut io),
            SyscallOutcome::Return(42)
        );
        assert_eq!(
            dispatch(SYS_EXIT, [0x123; 6], context, &mut io),
            SyscallOutcome::Exit { status: 0x23 }
        );
        assert_eq!(
            dispatch(SYS_EXIT_GROUP, [7; 6], context, &mut io),
            SyscallOutcome::Exit { status: 7 }
        );
    }

    #[test]
    fn clock_syscalls_copy_linux_timespec_and_reject_unsupported_clock_and_pointer() {
        let mut io = fake_io();
        assert_eq!(
            dispatch(
                SYS_CLOCK_GETTIME,
                [CLOCK_MONOTONIC, 0x4000, 0, 0, 0, 0],
                ProcessContext {
                    process_id: 1,
                    thread_id: 1,
                    uid: 0,
                    gid: 0,
                    supplementary_groups: [0; 8],
                    group_count: 0,
                    program_break: 0,
                },
                &mut io,
            ),
            SyscallOutcome::Return(0)
        );
        assert_eq!(&io.user[..8], &12i64.to_le_bytes());
        assert_eq!(&io.user[8..16], &340_000_000i64.to_le_bytes());

        assert_eq!(
            dispatch(
                SYS_CLOCK_GETRES,
                [CLOCK_MONOTONIC, 0x4010, 0, 0, 0, 0],
                ProcessContext {
                    process_id: 1,
                    thread_id: 1,
                    uid: 0,
                    gid: 0,
                    supplementary_groups: [0; 8],
                    group_count: 0,
                    program_break: 0,
                },
                &mut io,
            ),
            SyscallOutcome::Return(0)
        );
        assert_eq!(&io.user[0x10..0x18], &0i64.to_le_bytes());
        assert_eq!(&io.user[0x18..0x20], &10_000_000i64.to_le_bytes());

        let context = ProcessContext {
            process_id: 1,
            thread_id: 1,
            uid: 0,
            gid: 0,
            supplementary_groups: [0; 8],
            group_count: 0,
            program_break: 0,
        };
        assert_eq!(
            dispatch(
                SYS_CLOCK_GETTIME,
                [CLOCK_REALTIME, 0x4000, 0, 0, 0, 0],
                context,
                &mut io
            ),
            SyscallOutcome::Return((-(EINVAL as i64)) as u64)
        );
        assert_eq!(
            dispatch(
                SYS_CLOCK_GETTIME,
                [CLOCK_MONOTONIC, 0x5000, 0, 0, 0, 0],
                context,
                &mut io,
            ),
            SyscallOutcome::Return((-(EFAULT as i64)) as u64)
        );
    }

    #[test]
    fn openat_copies_a_bounded_absolute_path_from_user_memory() {
        let mut io = fake_io();
        let pathname = b"/etc/config\0";
        io.user[..pathname.len()].copy_from_slice(pathname);
        assert_eq!(
            dispatch(
                SYS_OPENAT,
                [AT_FDCWD, 0x4000, 0, 0, 0, 0],
                ProcessContext {
                    process_id: 1,
                    thread_id: 1,
                    uid: 1000,
                    gid: 1000,
                    supplementary_groups: [0; 8],
                    group_count: 0,
                    program_break: 0,
                },
                &mut io,
            ),
            SyscallOutcome::Return(42)
        );
        assert_eq!(io.opened_path, "/etc/config");
    }

    #[test]
    fn credential_syscalls_return_the_calling_process_identity() {
        let mut io = fake_io();
        let context = ProcessContext {
            process_id: 9,
            thread_id: 9,
            uid: 1201,
            gid: 3004,
            supplementary_groups: [0; 8],
            group_count: 0,
            program_break: 0,
        };
        for (number, expected) in [
            (SYS_GETUID, 1201),
            (SYS_GETEUID, 1201),
            (SYS_GETGID, 3004),
            (SYS_GETEGID, 3004),
        ] {
            assert_eq!(
                dispatch(number, [0; 6], context, &mut io),
                SyscallOutcome::Return(expected)
            );
        }
    }

    #[test]
    fn uname_writes_linux_x86_64_utsname_and_rejects_invalid_user_memory() {
        let mut io = fake_io();
        let context = ProcessContext {
            process_id: 9,
            thread_id: 9,
            uid: 1000,
            gid: 1000,
            supplementary_groups: [0; 8],
            group_count: 0,
            program_break: 0,
        };
        assert_eq!(
            dispatch(SYS_UNAME, [0x4000, 0, 0, 0, 0, 0], context, &mut io),
            SyscallOutcome::Return(0)
        );
        assert_eq!(&io.user[0..5], b"Linux");
        assert_eq!(&io.user[65..71], b"carlio");
        assert_eq!(&io.user[130..142], b"0.1.0-carlio");
        assert_eq!(&io.user[260..266], b"x86_64");

        assert_eq!(
            dispatch(SYS_UNAME, [0x9000, 0, 0, 0, 0, 0], context, &mut io),
            SyscallOutcome::Return((-(EFAULT as i64)) as u64)
        );
    }

    #[test]
    fn getgroups_copies_only_supplementary_gids_and_checks_capacity() {
        let mut io = fake_io();
        let context = ProcessContext {
            process_id: 9,
            thread_id: 9,
            uid: 1201,
            gid: 3004,
            supplementary_groups: [3005, 4006, 0, 0, 0, 0, 0, 0],
            group_count: 2,
            program_break: 0,
        };
        assert_eq!(
            dispatch(SYS_GETGROUPS, [0, 0, 0, 0, 0, 0], context, &mut io),
            SyscallOutcome::Return(2)
        );
        assert_eq!(
            dispatch(SYS_GETGROUPS, [1, 0x4000, 0, 0, 0, 0], context, &mut io),
            SyscallOutcome::Return((-(EINVAL as i64)) as u64)
        );
        assert_eq!(
            dispatch(SYS_GETGROUPS, [2, 0x4000, 0, 0, 0, 0], context, &mut io),
            SyscallOutcome::Return(2)
        );
        assert_eq!(&io.user[..8], &[0xbd, 0x0b, 0, 0, 0xa6, 0x0f, 0, 0]);
    }

    #[test]
    fn write_copies_user_bytes_and_returns_linux_byte_count() {
        let mut io = fake_io();
        let result = dispatch(
            SYS_WRITE,
            [1, 0x4000, 17, 0, 0, 0],
            ProcessContext {
                process_id: 1,
                thread_id: 1,
                uid: 0,
                gid: 0,
                supplementary_groups: [0; 8],
                group_count: 0,
                program_break: 0,
            },
            &mut io,
        );
        assert_eq!(result, SyscallOutcome::Return(17));
        assert_eq!(io.output, b"ferramenta linux\n");
    }

    #[test]
    fn write_reports_ebadf_efault_and_partial_progress() {
        let mut io = fake_io();
        assert_eq!(
            dispatch(
                SYS_WRITE,
                [9, 0x4000, 1, 0, 0, 0],
                ProcessContext {
                    process_id: 1,
                    thread_id: 1,
                    uid: 0,
                    gid: 0,
                    supplementary_groups: [0; 8],
                    group_count: 0,
                    program_break: 0
                },
                &mut io
            ),
            SyscallOutcome::Return((-(EBADF as i64)) as u64)
        );
        assert_eq!(
            dispatch(
                SYS_WRITE,
                [1, 0x9000, 1, 0, 0, 0],
                ProcessContext {
                    process_id: 1,
                    thread_id: 1,
                    uid: 0,
                    gid: 0,
                    supplementary_groups: [0; 8],
                    group_count: 0,
                    program_break: 0
                },
                &mut io
            ),
            SyscallOutcome::Return((-(EFAULT as i64)) as u64)
        );

        io.max_write = 5;
        assert_eq!(
            dispatch(
                SYS_WRITE,
                [1, 0x4000, 17, 0, 0, 0],
                ProcessContext {
                    process_id: 1,
                    thread_id: 1,
                    uid: 0,
                    gid: 0,
                    supplementary_groups: [0; 8],
                    group_count: 0,
                    program_break: 0
                },
                &mut io
            ),
            SyscallOutcome::Return(5)
        );
    }

    #[test]
    fn linux_socket_bind_and_listen_dispatch_validate_sockaddr_in() {
        let context = ProcessContext {
            process_id: 3,
            thread_id: 3,
            uid: 0,
            gid: 0,
            supplementary_groups: [0; 8],
            group_count: 0,
            program_break: 0,
        };
        let mut io = fake_io();
        assert_eq!(
            dispatch(SYS_SOCKET, [2, 1, 0, 0, 0, 0], context, &mut io),
            SyscallOutcome::Return(5)
        );
        io.user.fill(0);
        io.user[0..2].copy_from_slice(&2u16.to_le_bytes());
        io.user[2..4].copy_from_slice(&8080u16.to_be_bytes());
        io.user[4..8].copy_from_slice(&[127, 0, 0, 1]);
        assert_eq!(
            dispatch(SYS_BIND, [5, 0x4000, 16, 0, 0, 0], context, &mut io),
            SyscallOutcome::Return(0)
        );
        assert_eq!(
            dispatch(SYS_LISTEN, [5, 10, 0, 0, 0, 0], context, &mut io),
            SyscallOutcome::Return(0)
        );
        io.user[32..36].copy_from_slice(&4u32.to_le_bytes());
        assert_eq!(
            dispatch(
                SYS_GETSOCKNAME,
                [5, 0x4010, 0x4020, 0, 0, 0],
                context,
                &mut io
            ),
            SyscallOutcome::Return(0)
        );
        assert_eq!(&io.user[16..20], &[2, 0, 192, 0]);
        assert_eq!(&io.user[32..36], &16u32.to_le_bytes());
        assert_eq!(
            dispatch(SYS_LISTEN, [5, u64::MAX, 0, 0, 0, 0], context, &mut io),
            SyscallOutcome::Return((-(EINVAL as i64)) as u64)
        );
        io.user[0..2].copy_from_slice(&1u16.to_le_bytes());
        assert_eq!(
            dispatch(SYS_BIND, [5, 0x4000, 16, 0, 0, 0], context, &mut io),
            SyscallOutcome::Return((-(EAFNOSUPPORT as i64)) as u64)
        );
        assert_eq!(
            dispatch(SYS_BIND, [5, 0x4000, 8, 0, 0, 0], context, &mut io),
            SyscallOutcome::Return((-(EINVAL as i64)) as u64)
        );
    }

    #[test]
    fn read_copies_descriptor_bytes_to_user_and_rejects_bad_pointer() {
        let mut io = fake_io();
        let result = dispatch(
            SYS_READ,
            [1, 0x4000, 14, 0, 0, 0],
            ProcessContext {
                process_id: 7,
                thread_id: 7,
                uid: 0,
                gid: 0,
                supplementary_groups: [0; 8],
                group_count: 0,
                program_break: 0,
            },
            &mut io,
        );
        assert_eq!(result, SyscallOutcome::Return(14));
        assert_eq!(&io.user[..14], b"entrada linux\n");

        let mut io = fake_io();
        assert_eq!(
            dispatch(
                SYS_READ,
                [1, 0x9000, 4, 0, 0, 0],
                ProcessContext {
                    process_id: 7,
                    thread_id: 7,
                    uid: 0,
                    gid: 0,
                    supplementary_groups: [0; 8],
                    group_count: 0,
                    program_break: 0
                },
                &mut io,
            ),
            SyscallOutcome::Return((-(EFAULT as i64)) as u64)
        );
    }

    #[test]
    fn read_rejects_partial_invalid_buffer_before_consuming_input() {
        let mut io = fake_io();
        let result = dispatch(
            SYS_READ,
            [1, 0x4000 + io.user.len() as u64 - 1, 2, 0, 0, 0],
            ProcessContext {
                process_id: 7,
                thread_id: 7,
                uid: 0,
                gid: 0,
                supplementary_groups: [0; 8],
                group_count: 0,
                program_break: 0,
            },
            &mut io,
        );
        assert_eq!(result, SyscallOutcome::Return((-(EFAULT as i64)) as u64));
        assert_eq!(io.input_offset, 0);
    }

    #[test]
    fn unknown_linux_syscall_returns_enosys() {
        assert_eq!(
            dispatch(
                u64::MAX,
                [0; 6],
                ProcessContext {
                    process_id: 1,
                    thread_id: 1,
                    uid: 0,
                    gid: 0,
                    supplementary_groups: [0; 8],
                    group_count: 0,
                    program_break: 0
                },
                &mut fake_io()
            ),
            SyscallOutcome::Return((-(ENOSYS as i64)) as u64)
        );
    }
}

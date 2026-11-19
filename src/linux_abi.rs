//! Fatia inicial da ABI Linux x86_64, compartilhada entre as bases Arch/Ubuntu.
//!
//! O dispatcher mantém somente syscalls de console e ciclo de vida que podem
//! ser implementadas com os serviços explicitamente fornecidos pelo chamador.
//! Parte das chamadas entra pelo opcode `SYSCALL`; VFS e semânticas gerais
//! continuam sendo conectados por etapas.

pub const SYS_WRITE: u64 = 1;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_FSYNC: u64 = 74;
pub const SYS_LSEEK: u64 = 8;
pub const SYS_MMAP: u64 = 9;
pub const SYS_MPROTECT: u64 = 10;
pub const SYS_MUNMAP: u64 = 11;
pub const SYS_BRK: u64 = 12;
pub const SYS_READ: u64 = 0;
pub const SYS_OPEN: u64 = 2;
pub const SYS_FSTAT: u64 = 5;
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
pub const SYS_EXIT_GROUP: u64 = 231;
pub const SYS_SOCKET: u64 = 41;
pub const SYS_BIND: u64 = 49;
pub const SYS_LISTEN: u64 = 50;
pub const SYS_CLOCK_GETTIME: u64 = 228;
pub const SYS_CLOCK_GETRES: u64 = 229;
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
pub const AT_REMOVEDIR: u64 = 0x200;
pub const AT_SYMLINK_NOFOLLOW: u64 = 0x100;

pub const CLOCK_REALTIME: u64 = 0;
pub const CLOCK_MONOTONIC: u64 = 1;

pub const EBADF: u16 = 9;
pub const EPERM: u16 = 1;
pub const ENOENT: u16 = 2;
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
pub const ENFILE: u16 = 23;
pub const EOPNOTSUPP: u16 = 95;
pub const EROFS: u16 = 30;
pub const EEXIST: u16 = 17;
pub const EOVERFLOW: u16 = 75;
pub const ERANGE: u16 = 34;
pub const EAGAIN: u16 = 11;
pub const EBUSY: u16 = 16;

const WRITE_CHUNK_SIZE: usize = 256;
const PATH_BUFFER_SIZE: usize = crate::vfs::VFS_PATH_MAX;
const O_ACCMODE: u64 = 3;
const O_WRONLY: u64 = 1;
const O_RDWR: u64 = 2;
const O_CREAT: u64 = 0x40;
const O_EXCL: u64 = 0x80;
const O_TRUNC: u64 = 0x200;
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
    Exit { status: u8 },
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
    fn open(&mut self, _path: &str, _flags: u64, _mode: u64) -> Result<u64, u16> {
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
    fn bind(&mut self, _descriptor: u64, _address: [u8; 4], _port: u16) -> Result<(), u16> {
        Err(EOPNOTSUPP)
    }
    fn listen(&mut self, _descriptor: u64, _backlog: u16) -> Result<(), u16> {
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
        SYS_READ => read_syscall(arguments[0], arguments[1], arguments[2], io),
        SYS_OPEN => open_syscall(AT_FDCWD, arguments[0], arguments[1], arguments[2], io),
        SYS_FSTAT => fstat_syscall(arguments[0], arguments[1], io),
        SYS_ACCESS => access_syscall(arguments[0], arguments[1], io),
        SYS_NEWFSTATAT => {
            newfstatat_syscall(arguments[0], arguments[1], arguments[2], arguments[3], io)
        }
        SYS_OPENAT => open_syscall(arguments[0], arguments[1], arguments[2], arguments[3], io),
        SYS_CLOSE => result(io.close(arguments[0]).map(|_| 0)),
        SYS_FSYNC => result(io.fsync(arguments[0]).map(|_| 0)),
        SYS_SOCKET => result(io.socket(arguments[0], arguments[1], arguments[2])),
        SYS_BIND => bind_syscall(arguments[0], arguments[1], arguments[2], io),
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
                    Ok((path, absolute)) if arguments[0] == AT_FDCWD || absolute => {
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
    if flags != 0 {
        return errno(EOPNOTSUPP);
    }
    let mut path = [0u8; PATH_BUFFER_SIZE];
    let (path, absolute) = match read_rooted_path(path_address, &mut path, io) {
        Ok(path) => path,
        Err(error) => return errno(error),
    };
    if directory != AT_FDCWD && !absolute {
        return errno(EBADF);
    }
    let stat = match io.stat(path) {
        Ok(stat) => stat,
        Err(error) => return errno(error),
    };
    write_stat(stat_address, stat, io)
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
        Ok((path, absolute)) if directory == AT_FDCWD || absolute => {
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
        Ok((path, absolute)) if directory == AT_FDCWD || absolute => {
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
    let allowed = O_ACCMODE | O_CREAT | O_EXCL | O_TRUNC | O_APPEND;
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
    if dirfd != AT_FDCWD && !absolute {
        return errno(EBADF);
    }
    result(io.open(path, flags, mode))
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
    io: &mut I,
) -> SyscallOutcome {
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
        let amount = match io.read(descriptor, &mut buffer[..chunk]) {
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

fn write_syscall<I: LinuxSyscallIo>(
    descriptor: u64,
    address: u64,
    requested: u64,
    io: &mut I,
) -> SyscallOutcome {
    if requested == 0 {
        return SyscallOutcome::Return(0);
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
        }
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

//! Fatia inicial da ABI Linux x86_64, compartilhada entre as bases Arch/Ubuntu.
//!
//! O dispatcher mantém somente syscalls de console e ciclo de vida que podem
//! ser implementadas com os serviços explicitamente fornecidos pelo chamador.
//! Parte das chamadas entra pelo opcode `SYSCALL`; VFS e semânticas gerais
//! continuam sendo conectados por etapas.

pub const SYS_WRITE: u64 = 1;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_LSEEK: u64 = 8;
pub const SYS_MMAP: u64 = 9;
pub const SYS_MUNMAP: u64 = 11;
pub const SYS_BRK: u64 = 12;
pub const SYS_READ: u64 = 0;
pub const SYS_OPEN: u64 = 2;
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

pub const CLOCK_REALTIME: u64 = 0;
pub const CLOCK_MONOTONIC: u64 = 1;

pub const EBADF: u16 = 9;
pub const ENOENT: u16 = 2;
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

const WRITE_CHUNK_SIZE: usize = 256;
const PATH_BUFFER_SIZE: usize = crate::vfs::VFS_PATH_MAX;
const O_ACCMODE: u64 = 3;
const O_WRONLY: u64 = 1;
const O_RDWR: u64 = 2;
const O_CREAT: u64 = 0x40;
const O_TRUNC: u64 = 0x200;
const O_APPEND: u64 = 0x400;
const AT_FDCWD: u64 = (-100i64) as u64;
/// Limita o tempo máximo gasto em uma única chamada síncrona de console.
pub const MAX_WRITE_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessContext {
    pub process_id: u64,
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
        SYS_OPENAT => open_syscall(arguments[0], arguments[1], arguments[2], arguments[3], io),
        SYS_CLOSE => result(io.close(arguments[0]).map(|_| 0)),
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
        SYS_BRK => SyscallOutcome::Return(io.brk(context.program_break, arguments[0])),
        SYS_GETPID => SyscallOutcome::Return(context.process_id),
        SYS_GETTID => SyscallOutcome::Return(context.process_id),
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
    if dirfd != AT_FDCWD {
        return errno(EBADF);
    }
    if flags & O_CREAT != 0 {
        return errno(ENOSYS);
    }
    let allowed = O_ACCMODE | O_TRUNC | O_APPEND;
    if flags & !allowed != 0 || flags & O_ACCMODE == 3 {
        return errno(EOPNOTSUPP);
    }
    let writable = flags & O_ACCMODE == O_WRONLY || flags & O_ACCMODE == O_RDWR;
    if flags & O_TRUNC != 0 && !writable || flags & O_APPEND != 0 && !writable {
        return errno(EINVAL);
    }
    let mut bytes = [0u8; PATH_BUFFER_SIZE];
    let mut length = 0usize;
    let mut terminated = false;
    while length < bytes.len() {
        let location = match address.checked_add(length as u64) {
            Some(value) => value,
            None => return errno(EFAULT),
        };
        if io
            .copy_from_user(location, &mut bytes[length..length + 1])
            .is_err()
        {
            return errno(EFAULT);
        }
        if bytes[length] == 0 {
            terminated = true;
            break;
        }
        length += 1;
    }
    if !terminated || length == 0 {
        return errno(EINVAL);
    }
    let path = match core::str::from_utf8(&bytes[..length]) {
        Ok(path) if path.starts_with('/') => path,
        _ => return errno(EINVAL),
    };
    result(io.open(path, flags, mode))
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
            process_id: 42,
            uid: 1000,
            gid: 2000,
            supplementary_groups: [3000, 4000, 0, 0, 0, 0, 0, 0],
            group_count: 2,
            program_break: 0,
        };
        assert_eq!(
            dispatch(SYS_GETPID, [0; 6], context, &mut io),
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

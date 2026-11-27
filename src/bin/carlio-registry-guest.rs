//! Ponto de entrada do serviço de Registro no convidado.
//!
//! O supervisor fornece um canal privado como stdin/stdout e inicia uma
//! instância por hive. O processo termina quando o canal é fechado ou perde
//! o enquadramento. O formato do canal está em docs/REGISTRO-WINDOWS.md.

use carlio_os::userland::windows::registry::{
    Registry, RegistryError, Reply, MAX_REQUEST, MAX_VALUE,
};
use std::io::{self, Read, Write};

fn main() -> io::Result<()> {
    #[cfg(target_os = "linux")]
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--vm")) {
        return serve_vm();
    }
    serve(io::stdin().lock(), io::stdout().lock())
}

/// Configura a porta serial do convidado antes de aceitar dados binários.
#[cfg(target_os = "linux")]
fn serve_vm() -> io::Result<()> {
    use carlio_os::userland::windows::registry_vm::READY;
    use std::fs::OpenOptions;
    use std::os::fd::AsRawFd;

    if std::process::id() != 1 {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    // Os argumentos C são strings terminadas em NUL com vida estática.
    let mounted = unsafe {
        libc::mount(
            c"devtmpfs".as_ptr(),
            c"/dev".as_ptr(),
            c"devtmpfs".as_ptr(),
            libc::MS_NOSUID | libc::MS_NOEXEC,
            std::ptr::null(),
        )
    };
    if mounted != 0 {
        return Err(io::Error::last_os_error());
    }
    let mut port = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/ttyS0")?;
    let mut options = std::mem::MaybeUninit::uninit();
    // tcgetattr inicializa a estrutura em caso de sucesso.
    if unsafe { libc::tcgetattr(port.as_raw_fd(), options.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let mut options = unsafe { options.assume_init() };
    // A estrutura inicializada pertence à porta e é usada só nesta thread.
    unsafe { libc::cfmakeraw(&mut options) };
    if unsafe { libc::tcsetattr(port.as_raw_fd(), libc::TCSANOW, &options) } != 0 {
        return Err(io::Error::last_os_error());
    }
    port.write_all(&READY)?;
    serve(port.try_clone()?, port)
}

/// Processa uma mensagem por vez, com limites antes de ler o corpo.
fn serve(mut input: impl Read, mut output: impl Write) -> io::Result<()> {
    let mut registry = Registry::new();
    let mut length = [0; 2];
    let mut request = [0; MAX_REQUEST];
    let mut value = [0; MAX_VALUE];
    loop {
        match input.read(&mut length[..1]) {
            Ok(0) => return Ok(()),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
        input.read_exact(&mut length[1..])?;
        let request_len = usize::from(u16::from_le_bytes(length));
        let request = request.get_mut(..request_len).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "mensagem do Registro excedeu o limite",
            )
        })?;
        input.read_exact(request)?;

        let (status, kind, size, payload) = match registry.execute(request, &mut value) {
            Ok(Reply::Updated) => (0, 0, 0, &[][..]),
            Ok(Reply::Value { kind, len }) => (0, kind as u8, len, &value[..len]),
            Err(RegistryError::InvalidRequest) => (1, 0, 0, &[][..]),
            Err(RegistryError::NotFound) => (2, 0, 0, &[][..]),
            Err(RegistryError::HiveFull) => (3, 0, 0, &[][..]),
            Err(RegistryError::BufferTooSmall { .. }) => {
                return Err(io::Error::other("buffer interno do Registro insuficiente"));
            }
        };
        let size = (size as u16).to_le_bytes();
        output.write_all(&[status, kind, size[0], size[1]])?;
        output.write_all(payload)?;
        output.flush()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processes_multiple_frames_on_one_channel() -> io::Result<()> {
        let input = b"\x0b\0\x01\x02\x01\x01\x04KX\x2a\0\0\0\x07\0\x01\x01\x01\x01\0kx";
        let mut output = Vec::new();
        serve(&input[..], &mut output)?;
        assert_eq!(output, b"\0\0\0\0\0\x04\x04\0\x2a\0\0\0");
        Ok(())
    }

    #[test]
    fn closes_channels_with_oversized_or_truncated_frames() {
        for (input, kind) in [
            (&b"\xff\xff"[..], io::ErrorKind::InvalidData),
            (&b"\x07"[..], io::ErrorKind::UnexpectedEof),
            (&b"\x07\0\x01"[..], io::ErrorKind::UnexpectedEof),
        ] {
            let mut output = Vec::new();
            assert_eq!(serve(input, &mut output).unwrap_err().kind(), kind);
            assert!(output.is_empty());
        }
    }
}

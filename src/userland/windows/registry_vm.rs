//! Supervisor de referência para validar o convidado com QEMU em Linux.

use super::registry::{MAX_REQUEST, MAX_VALUE};
use super::registry_client::RegistryChannel;
use std::io::{self, Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Marcador enviado após o convidado preparar o canal binário.
pub const READY: [u8; 4] = *b"CRG1";

/// Uma VM por canal e hive. A queda do transporte encerra a VM.
pub struct RegistryVm {
    child: Child,
    channel: UnixStream,
}

impl RegistryVm {
    /// Inicia uma VM TCG com kernel e initramfs fornecidos pelo operador.
    pub fn boot(qemu: &Path, kernel: &Path, initramfs: &Path) -> io::Result<Self> {
        let (channel, serial) = UnixStream::pair()?;
        let input: OwnedFd = serial.try_clone()?.into();
        let output: OwnedFd = serial.into();
        let child = Command::new(qemu)
            .args([
                "-no-user-config",
                "-nodefaults",
                "-accel",
                "tcg",
                "-m",
                "256M",
                "-smp",
                "1",
                "-display",
                "none",
                "-nic",
                "none",
                "-monitor",
                "none",
                "-no-reboot",
                "-serial",
                "stdio",
                "-kernel",
            ])
            .arg(kernel)
            .arg("-initrd")
            .arg(initramfs)
            .args([
                "-append",
                "quiet loglevel=0 console=tty0 rdinit=/init -- --vm",
            ])
            .stdin(Stdio::from(input))
            .stdout(Stdio::from(output))
            .stderr(Stdio::null())
            .spawn()?;
        let mut vm = Self { child, channel };
        let mut ready = [0; READY.len()];
        vm.read(&mut ready, Instant::now() + Duration::from_secs(30))?;
        if ready != READY {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "assinatura do convidado inválida",
            ));
        }
        Ok(vm)
    }

    fn read(&mut self, mut bytes: &mut [u8], deadline: Instant) -> io::Result<()> {
        while !bytes.is_empty() {
            self.channel.set_read_timeout(Some(remaining(deadline)?))?;
            let count = self.channel.read(bytes)?;
            if count == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            bytes = &mut bytes[count..];
        }
        Ok(())
    }

    fn write(&mut self, mut bytes: &[u8], deadline: Instant) -> io::Result<()> {
        while !bytes.is_empty() {
            self.channel.set_write_timeout(Some(remaining(deadline)?))?;
            let count = self.channel.write(bytes)?;
            if count == 0 {
                return Err(io::ErrorKind::WriteZero.into());
            }
            bytes = &bytes[count..];
        }
        Ok(())
    }

    fn transact(
        &mut self,
        request: &[u8],
        response: &mut [u8; 4 + MAX_VALUE],
    ) -> io::Result<usize> {
        if request.len() > MAX_REQUEST {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        self.write(&(request.len() as u16).to_le_bytes(), deadline)?;
        self.write(request, deadline)?;
        self.read(&mut response[..4], deadline)?;
        let len = usize::from(u16::from_le_bytes([response[2], response[3]]));
        let body = response
            .get_mut(4..4 + len)
            .ok_or(io::ErrorKind::InvalidData)?;
        self.read(body, deadline)?;
        Ok(4 + len)
    }
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|time| !time.is_zero())
        .ok_or_else(|| io::ErrorKind::TimedOut.into())
}

impl RegistryChannel for RegistryVm {
    fn exchange(&mut self, request: &[u8], response: &mut [u8; 4 + MAX_VALUE]) -> Option<usize> {
        self.transact(request, response).ok()
    }
}

impl Drop for RegistryVm {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

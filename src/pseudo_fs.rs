//! Fontes somente leitura para os pseudo-filesystems `/proc` e `/sys`.
//!
//! Este módulo gera snapshots sem alocação. A montagem no VFS e a coleta dos
//! dados reais de scheduler/memória são responsabilidade do kernel.

use core::fmt::{self, Write};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PseudoFsError {
    NotFound,
    BufferTooSmall,
    InvalidOffset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemSnapshot {
    pub process_count: usize,
    pub online_cpus: usize,
    pub total_memory_bytes: u64,
    pub free_memory_bytes: u64,
    pub uptime_seconds: u64,
    pub kernel_name: &'static str,
    pub kernel_release: &'static str,
}

/// Copia a faixa `[offset, offset + output.len())` do arquivo virtual.
/// Retorna 0 ao atingir EOF; o arquivo completo nunca precisa ser alocado.
pub fn read(
    snapshot: SystemSnapshot,
    path: &str,
    offset: usize,
    output: &mut [u8],
) -> Result<usize, PseudoFsError> {
    let mut text = TextBuffer::<512>::new();
    match path {
        "/proc/uptime" => writeln!(
            text,
            "{}.00 {}.00",
            snapshot.uptime_seconds, snapshot.uptime_seconds
        )
        .unwrap(),
        "/proc/loadavg" => writeln!(text, "0.00 0.00 0.00 0/{} 1", snapshot.process_count).unwrap(),
        "/proc/meminfo" => {
            writeln!(text, "MemTotal: {} kB", snapshot.total_memory_bytes / 1024).unwrap();
            writeln!(text, "MemFree: {} kB", snapshot.free_memory_bytes / 1024).unwrap();
            writeln!(
                text,
                "MemAvailable: {} kB",
                snapshot.free_memory_bytes / 1024
            )
            .unwrap();
        }
        "/proc/cpuinfo" => {
            for cpu in 0..snapshot.online_cpus {
                writeln!(text, "processor\t: {cpu}\nvendor_id\t: CarlioOS\nmodel name\t: CarlioOS virtual CPU\n").unwrap();
            }
        }
        "/proc/version" => writeln!(
            text,
            "{} version {}",
            snapshot.kernel_name, snapshot.kernel_release
        )
        .unwrap(),
        "/sys/kernel/uevent_seqnum" => writeln!(text, "0").unwrap(),
        "/sys/devices/system/cpu/online" => {
            if snapshot.online_cpus == 0 {
                writeln!(text).unwrap();
            } else if snapshot.online_cpus == 1 {
                writeln!(text, "0").unwrap();
            } else {
                writeln!(text, "0-{}", snapshot.online_cpus - 1).unwrap();
            }
        }
        _ => return Err(PseudoFsError::NotFound),
    }
    if offset > text.len {
        return Err(PseudoFsError::InvalidOffset);
    }
    let count = output.len().min(text.len - offset);
    output[..count].copy_from_slice(&text.bytes[offset..offset + count]);
    Ok(count)
}

struct TextBuffer<const N: usize> {
    bytes: [u8; N],
    len: usize,
}
impl<const N: usize> TextBuffer<N> {
    const fn new() -> Self {
        Self {
            bytes: [0; N],
            len: 0,
        }
    }
}
impl<const N: usize> Write for TextBuffer<N> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let end = self.len.checked_add(value.len()).ok_or(fmt::Error)?;
        let destination = self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?;
        destination.copy_from_slice(value.as_bytes());
        self.len = end;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> SystemSnapshot {
        SystemSnapshot {
            process_count: 3,
            online_cpus: 4,
            total_memory_bytes: 8 * 1024 * 1024,
            free_memory_bytes: 2 * 1024 * 1024,
            uptime_seconds: 12,
            kernel_name: "CarlioOS",
            kernel_release: "0.1",
        }
    }

    #[test]
    fn exposes_proc_and_sys_snapshots_and_supports_offsets() {
        let mut full = [0; 64];
        let len = read(snapshot(), "/proc/meminfo", 0, &mut full).unwrap();
        assert!(core::str::from_utf8(&full[..len])
            .unwrap()
            .contains("MemTotal: 8192 kB"));
        let mut tail = [0; 8];
        let count = read(snapshot(), "/sys/devices/system/cpu/online", 2, &mut tail).unwrap();
        assert_eq!(&tail[..count], b"3\n");
        assert_eq!(
            read(snapshot(), "/proc/uptime", 999, &mut tail),
            Err(PseudoFsError::InvalidOffset)
        );
    }

    #[test]
    fn rejects_unknown_paths_and_bounds_cpu_output() {
        let mut out = [0; 512];
        assert_eq!(
            read(snapshot(), "/proc/secret", 0, &mut out),
            Err(PseudoFsError::NotFound)
        );
        let count = read(snapshot(), "/proc/cpuinfo", 0, &mut out).unwrap();
        assert!(count > 0);
    }
}

//! Boot-time restrictions shared by the Linux ABI and kernel diagnostics.

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum Level {
    None,
    Integrity,
    Confidentiality,
}

impl Level {
    pub const fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::None,
            1 => Self::Integrity,
            _ => Self::Confidentiality,
        }
    }

    pub fn parse(value: &[u8]) -> Option<Self> {
        match value {
            b"none" => Some(Self::None),
            b"integrity" => Some(Self::Integrity),
            b"confidentiality" => Some(Self::Confidentiality),
            _ => None,
        }
    }

    pub fn from_boot(floor: Self, command_line: &[u8]) -> Self {
        command_line
            .split(u8::is_ascii_whitespace)
            .filter_map(|arg| arg.strip_prefix(b"lockdown="))
            .map(|arg| Self::parse(arg).unwrap_or(Self::Confidentiality))
            .fold(floor, core::cmp::max)
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Integrity => "integrity",
            Self::Confidentiality => "confidentiality",
        }
    }

    pub fn restricts_integrity(self) -> bool {
        self >= Self::Integrity
    }

    pub fn hides_diagnostics(self) -> bool {
        self >= Self::Confidentiality
    }

    pub fn denies_syscall(self, number: u64, arguments: [u64; 6]) -> bool {
        if !self.restricts_integrity() {
            return false;
        }
        match number {
            // iopl, ioperm, init_module, delete_module, kexec_load,
            // finit_module, kexec_file_load (Linux x86-64).
            172 | 173 | 175 | 176 | 246 | 313 | 320 => true,
            // reboot(KEXEC) and reboot(SW_SUSPEND).
            169 => matches!(arguments[2] as u32, 0x4558_4543 | 0xd000_fce2),
            // syslog, perf_event_open, bpf.
            103 | 298 | 321 => self.hides_diagnostics(),
            _ => false,
        }
    }

    pub fn denies_path(self, path: &str) -> bool {
        if !self.restricts_integrity() {
            return false;
        }
        // VFS accepts repeated slashes and dot components. FAT lookups can
        // also ignore case. Match the resolved spelling, not the raw path.
        let mut parts = [""; crate::vfs::VFS_PATH_MAX / 2 + 1];
        let mut count = 0usize;
        for part in path.split('/') {
            match part {
                "" | "." => {}
                ".." => count = count.saturating_sub(1),
                _ => {
                    let Some(slot) = parts.get_mut(count) else {
                        return true;
                    };
                    *slot = part;
                    count += 1;
                }
            }
        }
        let at_or_below = |root: &&str| {
            let mut requested = parts[..count].iter();
            root.split('/').skip(1).all(|component| {
                requested
                    .next()
                    .is_some_and(|part| component.eq_ignore_ascii_case(part))
            })
        };

        [
            "/dev/mem",
            "/dev/kmem",
            "/dev/port",
            "/dev/cpu",
            "/sys/bus/pci",
        ]
        .iter()
        .any(at_or_below)
            || self.hides_diagnostics()
                && [
                    "/proc/kcore",
                    "/proc/kallsyms",
                    "/dev/crash",
                    "/sys/kernel/debug",
                    "/sys/kernel/tracing",
                ]
                .iter()
                .any(at_or_below)
    }
}

#[cfg(test)]
mod tests {
    use super::Level::*;

    #[test]
    fn boot_floor_and_strictest_argument_win() {
        assert_eq!(
            super::Level::from_boot(Integrity, b"lockdown=none"),
            Integrity
        );
        assert_eq!(
            super::Level::from_boot(None, b"x=2 lockdown=integrity lockdown=none"),
            Integrity
        );
        assert_eq!(
            super::Level::from_boot(None, b"lockdown=typo"),
            Confidentiality
        );
        assert_eq!(
            super::Level::from_boot(None, b"otherlockdown=integrity"),
            None
        );
    }

    #[test]
    fn levels_distinguish_integrity_from_confidentiality() {
        for level in [Integrity, Confidentiality] {
            assert!(level.denies_path("/dev/mem"));
            assert!(level.denies_path("/sys/bus/pci/devices"));
            assert!(!level.denies_path("/dev/memory"));
            assert!(level.denies_syscall(313, [0; 6]));
            assert!(level.denies_syscall(246, [0; 6]));
            assert!(level.denies_syscall(169, [0, 0, 0xd000_fce2, 0, 0, 0]));
            assert!(!level.denies_syscall(1, [0; 6]));
        }
        assert!(Integrity.denies_path("//dev/./../DEV//mem"));
        assert!(!None.denies_path("/dev/mem"));
        assert!(!None.denies_syscall(313, [0; 6]));
        assert!(!Integrity.denies_path("/proc/kcore"));
        assert!(Confidentiality.denies_path("/proc/kcore"));
        assert!(Confidentiality.denies_syscall(298, [0; 6]));
    }
}

//! Kernel policy for the dircache service and audit reader.
pub const AUDIT_PATH: &str = "/dev/carlio/audit";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Domain {
    #[default]
    Ordinary,
    DirectoryBackend,
}

impl Domain {
    pub const fn permits_registration(self, capabilities: u64) -> bool {
        matches!(self, Self::DirectoryBackend) && capabilities & CAP_DAC_OVERRIDE != 0
    }
}

pub const CAP_DAC_OVERRIDE: u64 = 1 << 1;

pub fn executable_domain(path: &str, program: &[u8], has_interpreter: bool) -> Domain {
    let approved = include_bytes!(concat!(env!("OUT_DIR"), "/dircache-backend.elf"));
    if cfg!(feature = "dircache")
        && !has_interpreter
        && path == "/usr/sbin/pegasusd"
        && !approved.is_empty()
        && program == approved
    {
        Domain::DirectoryBackend
    } else {
        Domain::Ordinary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_fixture_is_bound_to_bytes_path_and_static_loading() {
        let approved = include_bytes!(concat!(env!("OUT_DIR"), "/dircache-backend.elf"));
        if !cfg!(feature = "dircache") || approved.is_empty() {
            return;
        }
        assert_eq!(
            executable_domain("/usr/sbin/pegasusd", approved, false),
            Domain::DirectoryBackend
        );
        assert_eq!(
            executable_domain("/tmp/pegasusd", approved, false),
            Domain::Ordinary
        );
        assert_eq!(
            executable_domain("/usr/sbin/pegasusd", approved, true),
            Domain::Ordinary
        );
        let mut changed = approved.to_vec();
        changed[0] ^= 1;
        assert_eq!(
            executable_domain("/usr/sbin/pegasusd", &changed, false),
            Domain::Ordinary
        );
    }

    #[test]
    fn registration_requires_both_capability_and_domain() {
        assert!(!Domain::Ordinary.permits_registration(CAP_DAC_OVERRIDE));
        assert!(!Domain::DirectoryBackend.permits_registration(0));
        assert!(Domain::DirectoryBackend.permits_registration(CAP_DAC_OVERRIDE));
        assert_eq!(
            executable_domain("/usr/sbin/pegasusd", b"unknown", false),
            Domain::Ordinary
        );
    }
}

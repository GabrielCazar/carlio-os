//! Kernel policy for the dircache service and audit reader.
pub const AUDIT_PATH: &str = "/dev/carlio/audit";
pub const BACKEND_PATH: &str = "/usr/sbin/pegasusd";
pub const POSIX_MAPPING_PATH: &str = "/etc/pegasusds/identities.tsv";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Domain {
    #[default]
    Ordinary,
    DirectoryBackend,
}

impl Domain {
    pub const fn permits_registration(self, capabilities: u64) -> bool {
        matches!(self, Self::DirectoryBackend) && capabilities & CAP_DIRCACHE_REGISTER != 0
    }
}

pub(crate) const CAP_DIRCACHE_REGISTER: u64 = 1;

pub fn supervised_backend_allowed(uid: u32, arguments: &[&str]) -> bool {
    let service_uid = env!("CARLIO_DIRCACHE_SERVICE_UID").parse().unwrap_or(0);
    matches_supervised_backend(service_uid, uid, arguments)
}

fn matches_supervised_backend(service_uid: u32, uid: u32, arguments: &[&str]) -> bool {
    service_uid != 0
        && uid == service_uid
        && arguments == [BACKEND_PATH, "--dircache", POSIX_MAPPING_PATH]
}

pub fn approved_backend() -> Option<&'static [u8]> {
    let image = include_bytes!(concat!(env!("OUT_DIR"), "/dircache-backend.elf"));
    (cfg!(feature = "dircache") && !image.is_empty()).then_some(image)
}

pub fn service_syscall_allowed(number: u64, args: [u64; 6]) -> bool {
    use crate::linux_abi::*;
    const SYS_SET_TID_ADDRESS: u64 = 218;
    const SYS_SET_ROBUST_LIST: u64 = 273;
    const SYS_PRLIMIT64: u64 = 302;
    const SYS_RSEQ: u64 = 334;

    match number {
        SYS_SOCKET => args[0] == 1 && args[1] & !(O_CLOEXEC | 0x800) == 5 && args[2] == 0,
        SYS_WRITE if args[0] < 3 => true,
        SYS_SENDTO | SYS_RECVFROM | SYS_WRITE | SYS_BIND | SYS_LISTEN | SYS_ACCEPT
        | SYS_ACCEPT4 => crate::dircache_socket::is_descriptor(args[0]),
        SYS_MMAP => args[2] & !3 == 0 && args[3] == 0x22 && args[4] as u32 == u32::MAX,
        SYS_MPROTECT => args[2] & !3 == 0,
        SYS_ARCH_PRCTL | SYS_READ | SYS_OPEN | SYS_OPENAT | SYS_CLOSE | SYS_FSTAT | SYS_POLL
        | SYS_PPOLL | SYS_BRK | SYS_MUNMAP | SYS_MADVISE | SYS_GETRANDOM | SYS_CLOCK_GETTIME
        | SYS_CLOCK_GETRES | SYS_GETPID | SYS_GETTID | SYS_GETUID | SYS_GETGID | SYS_GETEUID
        | SYS_GETEGID | SYS_RT_SIGACTION | SYS_FUTEX | SYS_EXIT | SYS_EXIT_GROUP => true,
        // libc uses these optional calls during static startup. The ABI
        // dispatcher returns ENOSYS for unimplemented calls.
        SYS_SET_TID_ADDRESS | SYS_SET_ROBUST_LIST | SYS_PRLIMIT64 | SYS_RSEQ => true,
        _ => false,
    }
}

pub fn service_open_allowed(path: &str, flags: u64) -> bool {
    path == POSIX_MAPPING_PATH && flags & !crate::linux_abi::O_CLOEXEC == 0
}

pub fn executable_domain(path: &str, program: &[u8], has_interpreter: bool) -> Domain {
    let approved = include_bytes!(concat!(env!("OUT_DIR"), "/dircache-backend.elf"));
    if cfg!(feature = "dircache")
        && !has_interpreter
        && path == BACKEND_PATH
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
    fn service_can_open_only_the_mapping_read_only() {
        use crate::linux_abi::O_CLOEXEC;
        const O_WRONLY: u64 = 1;
        const O_CREAT: u64 = 0x40;
        assert!(service_open_allowed(POSIX_MAPPING_PATH, O_CLOEXEC));
        assert!(!service_open_allowed(POSIX_MAPPING_PATH, O_WRONLY));
        assert!(!service_open_allowed(POSIX_MAPPING_PATH, O_CREAT));
        assert!(!service_open_allowed("/dev/null", 0));
    }

    #[test]
    fn service_policy_denies_process_creation_network_and_executable_memory() {
        use crate::linux_abi::*;
        let empty = [0; 6];
        for number in [
            SYS_EXECVE,
            SYS_FORK,
            SYS_CLONE,
            SYS_CONNECT,
            SYS_SETUID,
            SYS_UNLINK,
        ] {
            assert!(!service_syscall_allowed(number, empty));
        }
        assert!(!service_syscall_allowed(SYS_WRITE, [3, 0, 0, 0, 0, 0]));
        assert!(!service_syscall_allowed(SYS_SOCKET, [2, 5, 0, 0, 0, 0]));
        assert!(service_syscall_allowed(
            SYS_SOCKET,
            [1, 5 | O_CLOEXEC, 0, 0, 0, 0]
        ));
        assert!(!service_syscall_allowed(
            SYS_MPROTECT,
            [0, 4096, 5, 0, 0, 0]
        ));
        assert!(service_syscall_allowed(
            SYS_MMAP,
            [0, 4096, 3, 0x22, u64::MAX, 0]
        ));
    }

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
        assert!(!Domain::Ordinary.permits_registration(CAP_DIRCACHE_REGISTER));
        assert!(!Domain::DirectoryBackend.permits_registration(0));
        assert!(Domain::DirectoryBackend.permits_registration(CAP_DIRCACHE_REGISTER));
        assert_eq!(
            executable_domain("/usr/sbin/pegasusd", b"unknown", false),
            Domain::Ordinary
        );
    }

    #[test]
    fn supervised_registration_requires_configured_identity_and_fixed_arguments() {
        let approved = [BACKEND_PATH, "--dircache", POSIX_MAPPING_PATH];
        assert!(matches_supervised_backend(1000, 1000, &approved));
        assert!(!matches_supervised_backend(0, 0, &approved));
        assert!(!matches_supervised_backend(1000, 1001, &approved));
        assert!(!matches_supervised_backend(1000, 1000, &approved[..2]));
        assert!(!matches_supervised_backend(
            1000,
            1000,
            &[BACKEND_PATH, "--dircache", "/tmp/identities.tsv"]
        ));
    }
}

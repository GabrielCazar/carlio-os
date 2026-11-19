use super::Session;
use crate::vfs::{AccessContext, Vfs, VfsError};

#[derive(Clone, Copy)]
struct AppEntry {
    name: &'static str,
    executable: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchError {
    InvalidName,
    AlreadyRegistered,
    CatalogFull,
    NotFound,
    AccessDenied,
    LoadFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramLoadError {
    Failed,
}

pub trait ProgramLoader {
    fn start(&mut self, executable: &str, session: Session) -> Result<(), ProgramLoadError>;
}

/// Catálogo por nome; valida acesso no VFS antes de chamar o carregador.
pub struct AppCatalog<const N: usize> {
    entries: [Option<AppEntry>; N],
    len: usize,
}

impl<const N: usize> AppCatalog<N> {
    pub const fn new() -> Self {
        Self {
            entries: [None; N],
            len: 0,
        }
    }

    pub fn register(
        &mut self,
        name: &'static str,
        executable: &'static str,
    ) -> Result<(), LaunchError> {
        if name.is_empty()
            || name.len() > 31
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || !executable.starts_with("/programas/")
        {
            return Err(LaunchError::InvalidName);
        }
        if self.entries[..self.len]
            .iter()
            .flatten()
            .any(|entry| entry.name == name)
        {
            return Err(LaunchError::AlreadyRegistered);
        }
        if self.len == N {
            return Err(LaunchError::CatalogFull);
        }
        self.entries[self.len] = Some(AppEntry { name, executable });
        self.len += 1;
        Ok(())
    }

    pub fn launch<const V: usize, L: ProgramLoader>(
        &self,
        name: &str,
        session: Session,
        filesystem: &Vfs<V>,
        loader: &mut L,
    ) -> Result<(), LaunchError> {
        let entry = self.entries[..self.len]
            .iter()
            .flatten()
            .find(|entry| entry.name == name)
            .ok_or(LaunchError::NotFound)?;
        filesystem
            .authorize_execution_as(
                entry.executable,
                AccessContext::new(session.credentials(), session.supplementary_groups(), 0),
            )
            .map_err(|error| match error {
                VfsError::NotFound => LaunchError::NotFound,
                _ => LaunchError::AccessDenied,
            })?;
        loader
            .start(entry.executable, session)
            .map_err(|_| LaunchError::LoadFailed)
    }
}

impl<const N: usize> Default for AppCatalog<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        userland::login::{CredentialVerifier, LoginManager},
        vfs::{Credentials, NodeKind},
    };

    struct Verifier;
    impl CredentialVerifier for Verifier {
        fn verify(&self, _: &str, _: &[u8]) -> Option<Credentials> {
            Some(Credentials {
                uid: 1000,
                gid: 1000,
            })
        }

        fn supplementary_groups(&self, _: &str) -> ([u32; 8], usize) {
            ([3000, 0, 0, 0, 0, 0, 0, 0], 1)
        }
    }
    #[derive(Default)]
    struct Loader(Option<&'static str>);
    impl ProgramLoader for Loader {
        fn start(&mut self, path: &str, _: Session) -> Result<(), ProgramLoadError> {
            self.0 = Some(if path == "/programas/editor.elf" {
                "/programas/editor.elf"
            } else {
                "invalid"
            });
            Ok(())
        }
    }

    #[test]
    fn catalog_checks_vfs_execute_permission_before_dispatch() {
        let session = LoginManager::new(Verifier)
            .authenticate("alice", b"x")
            .unwrap();
        let root = Credentials { uid: 0, gid: 0 };
        let mut vfs = Vfs::<8>::new();
        vfs.create_dir("/", "programas", root, 0o2755).unwrap();
        vfs.set_owner("/programas", root, 0, 3000).unwrap();
        vfs.create_file("/programas", "editor.elf", root, 0o0710)
            .unwrap();
        vfs.set_owner("/programas/editor.elf", root, 0, 3000)
            .unwrap();
        assert_eq!(
            vfs.check_access_as(
                "/programas/editor.elf",
                AccessContext::new(session.credentials(), session.supplementary_groups(), 0),
                crate::vfs::Access::EXECUTE
            ),
            Ok(NodeKind::File)
        );
        let mut apps = AppCatalog::<2>::new();
        apps.register("editor", "/programas/editor.elf").unwrap();
        let mut loader = Loader::default();
        assert_eq!(apps.launch("editor", session, &vfs, &mut loader), Ok(()));
        assert_eq!(loader.0, Some("/programas/editor.elf"));
    }
}

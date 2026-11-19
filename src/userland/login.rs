use crate::vfs::Credentials;

const GROUPS_MAX: usize = 8;

pub const LOGIN_PROMPT: &str = "CarlioOS — entrar\nUsuário: ";
pub const PASSWORD_PROMPT: &str = "Senha: ";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginError {
    InvalidInput,
    InvalidCredentials,
}

/// O backend deve validar credenciais sem registrar senha em texto claro.
pub trait CredentialVerifier {
    fn verify(&mut self, username: &str, password: &[u8]) -> Option<Credentials>;

    /// Retorna até oito grupos suplementares; o backend deve fornecer apenas
    /// grupos atribuídos à conta após autenticação.
    fn supplementary_groups(&self, _username: &str) -> ([u32; GROUPS_MAX], usize) {
        ([0; GROUPS_MAX], 0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Session {
    credentials: Credentials,
    groups: [u32; GROUPS_MAX],
    group_count: usize,
}

impl Session {
    pub const fn credentials(self) -> Credentials {
        self.credentials
    }

    pub fn supplementary_groups(&self) -> &[u32] {
        &self.groups[..self.group_count]
    }
}

pub struct LoginManager<V> {
    verifier: V,
}

impl<V: CredentialVerifier> LoginManager<V> {
    pub const fn new(verifier: V) -> Self {
        Self { verifier }
    }

    pub fn authenticate(&mut self, username: &str, password: &[u8]) -> Result<Session, LoginError> {
        if username.is_empty()
            || username.len() > 31
            || !username
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || password.is_empty()
            || password.len() > 128
        {
            return Err(LoginError::InvalidInput);
        }
        let credentials = self
            .verifier
            .verify(username, password)
            .ok_or(LoginError::InvalidCredentials)?;
        let (groups, group_count) = self.verifier.supplementary_groups(username);
        if group_count > GROUPS_MAX {
            return Err(LoginError::InvalidCredentials);
        }
        Ok(Session {
            credentials,
            groups,
            group_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestVerifier;
    impl CredentialVerifier for TestVerifier {
        fn verify(&mut self, username: &str, password: &[u8]) -> Option<Credentials> {
            (username == "alice" && password == b"correct horse").then_some(Credentials {
                uid: 1000,
                gid: 1000,
            })
        }
    }

    #[test]
    fn creates_session_only_after_verifier_accepts_credentials() {
        let mut login = LoginManager::new(TestVerifier);
        let session = login.authenticate("alice", b"correct horse").unwrap();
        assert_eq!(
            session.credentials(),
            Credentials {
                uid: 1000,
                gid: 1000
            }
        );
        assert_eq!(
            login.authenticate("alice", b"wrong"),
            Err(LoginError::InvalidCredentials)
        );
        assert_eq!(
            login.authenticate("../alice", b"secret"),
            Err(LoginError::InvalidInput)
        );
    }
}

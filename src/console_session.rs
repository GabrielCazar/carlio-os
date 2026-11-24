//! Sessions on the trusted management console.
use super::*;
use carlio_os::userland::{
    accounts::{AccountDatabase, AccountVerifier, LOGIN_MEMORY_KIB, RESERVED_TEST_ACCOUNT},
    kdf::Argon2idVerifier,
    login::{LoginManager, PASSWORD_LIMIT, Session},
};
use zeroize::Zeroizing;

static NETWORK: IrqSpinLock<Option<(e1000::DhcpLease, u64)>> = IrqSpinLock::new(None);

pub(super) fn configure_network(lease: e1000::DhcpLease) {
    *NETWORK.lock() = Some((lease, idt::uptime_ticks()));
}

pub(super) fn current_lease() -> Option<e1000::DhcpLease> {
    let (lease, start) = (*NETWORK.lock())?;
    let duration = u64::from(lease.lease_seconds?) * idt::TICK_RATE_HZ;
    (idt::uptime_ticks().wrapping_sub(start) < duration).then_some(lease)
}

pub(super) struct Console {
    session: Option<Session>,
    after_cr: bool,
    status_enabled: bool,
}

impl Console {
    pub(super) const fn new() -> Self {
        Self {
            session: None,
            after_cr: false,
            status_enabled: false,
        }
    }

    pub(super) fn prompt(&self) -> &'static str {
        if self.session.is_some() {
            "sessao> "
        } else {
            "carlio> "
        }
    }

    pub(super) fn read_line(&mut self, line: &mut [u8], echo: bool) -> Option<usize> {
        let mut len = 0;
        let mut overflow = false;
        loop {
            if self.status_enabled {
                if let Some(lease) = current_lease() {
                    e1000::poll_status(lease);
                } else {
                    self.status_enabled = false;
                }
            }
            let Ok(byte) = serial_read() else { continue };
            // Serial clients may send CRLF. Consume the LF without submitting another line.
            let paired_lf = self.after_cr && byte == b'\n';
            self.after_cr = byte == b'\r';
            if paired_lf {
                continue;
            }
            match byte {
                b'\r' | b'\n' => {
                    write("\r\n");
                    return (!overflow).then_some(len);
                }
                8 | 127 if len != 0 => {
                    len -= 1;
                    line[len] = 0;
                    if echo {
                        write("\x08 \x08");
                    }
                }
                value if (32..=126).contains(&value) || (!echo && value >= 128) => {
                    if len == line.len() {
                        if !overflow {
                            overflow = true;
                            record_error(KernelError::InputTooLong);
                        }
                        continue;
                    }
                    line[len] = byte;
                    len += 1;
                    if echo {
                        serial_write(byte);
                    }
                }
                _ => {}
            }
        }
    }

    pub(super) fn handle(&mut self, command: &str) -> bool {
        // Authenticated commands run under the session identity and never fall through
        // to the management command dispatcher.
        if let Some(session) = self.session {
            if command == "sair" {
                self.session = None;
                write("Sessão encerrada.\r\n");
            } else {
                session_command(session, command);
            }
            return true;
        }
        match command {
            "rede-status iniciar" => {
                self.status_enabled =
                    current_lease().is_some_and(|lease| lease.subnet_mask.is_some());
                write(if self.status_enabled {
                    "Rede: status UDP ativo na porta 7704.\r\n"
                } else {
                    "Rede: endereço DHCP indisponível.\r\n"
                });
            }
            "rede-status parar" => {
                self.status_enabled = false;
                write("Rede: status UDP parado.\r\n");
            }
            _ => {
                let Some(name) = command.strip_prefix("login ") else {
                    return false;
                };
                write("Senha: ");
                let mut password = Zeroizing::new([0; PASSWORD_LIMIT]);
                let session = self
                    .read_line(&mut *password, false)
                    .and_then(|len| authenticate(name, &password[..len]));
                drop(password);
                self.session = session;
                if session.is_some() {
                    write("Sessão autenticada. Digite ajuda.\r\n");
                } else {
                    idt::wait_ticks(100);
                    write("Credenciais inválidas ou conta indisponível.\r\n");
                }
            }
        }
        true
    }
}

fn authenticate(name: &str, password: &[u8]) -> Option<Session> {
    if name == RESERVED_TEST_ACCOUNT
        || VFS_STORE_FAILED.load(Ordering::Acquire)
        || VFS_STORE.lock().is_none()
    {
        return None;
    }
    let database = AccountDatabase::load_protected(&PROCESS_VFS.lock()).ok()?;
    let mut memory = alloc::vec::Vec::new();
    memory.try_reserve_exact(LOGIN_MEMORY_KIB).ok()?;
    memory.resize(LOGIN_MEMORY_KIB, argon2::Block::new());
    let mut login = LoginManager::new(AccountVerifier::new(
        &database,
        Argon2idVerifier::new(&mut memory),
    ));
    let session = login.authenticate(name, password).ok()?;
    let credentials = session.credentials();
    (credentials.uid != 0
        && credentials.uid != u32::MAX
        && credentials.gid != 0
        && credentials.gid != u32::MAX
        && !session
            .supplementary_groups()
            .iter()
            .any(|&group| group == 0 || group == u32::MAX))
    .then_some(session)
}

fn session_command(session: Session, command: &str) {
    let credentials = session.credentials();
    let context = AccessContext::new(credentials, session.supplementary_groups(), 0o077);
    if command == "id" {
        write("uid=");
        write_u32(credentials.uid);
        write(" gid=");
        write_u32(credentials.gid);
        for &group in session.supplementary_groups() {
            write(" grupo=");
            write_u32(group);
        }
        write("\r\n");
        return;
    }
    if let Some(name) = command.strip_prefix("resolver ") {
        let Some(lease) = current_lease() else {
            write("DNS IPv4: endereço DHCP indisponível.\r\n");
            return;
        };
        match e1000::resolve_dns_a(lease, name) {
            Ok(Some(address)) => {
                write("DNS IPv4: ");
                for (index, octet) in address.into_iter().enumerate() {
                    if index != 0 {
                        write(".");
                    }
                    write_u32(u32::from(octet));
                }
                write("\r\n");
            }
            Ok(None) => write("DNS IPv4: sem resposta A válida.\r\n"),
            Err(error) => {
                write("DNS IPv4: ");
                write(error.message());
                write(".\r\n");
            }
        }
        return;
    }
    if let Some(path) = command.strip_prefix("ler ") {
        let mut bytes = [0; 1024];
        let result = PROCESS_VFS.lock().read_file_as(path, context, &mut bytes);
        match result {
            Ok(len) => {
                for &byte in &bytes[..len] {
                    serial_write(if byte.is_ascii_graphic() || byte == b' ' {
                        byte
                    } else {
                        b'.'
                    });
                }
                write("\r\n");
            }
            Err(error) => show_vfs_error(vfs_errno(error)),
        }
        return;
    }
    if let Some(path) = command.strip_prefix("criar ") {
        if let Some((parent, name)) = path.rsplit_once('/') {
            let parent = if parent.is_empty() { "/" } else { parent };
            mutate(|vfs| vfs.create_file_as(parent, name, context, 0o666));
            return;
        }
    }
    if let Some((path, data)) = command
        .strip_prefix("gravar ")
        .and_then(|args| args.split_once(' '))
    {
        mutate(|vfs| vfs.write_file_as(path, context, data.as_bytes()));
        return;
    }
    write("Sessão: id, ler CAMINHO, criar CAMINHO, gravar CAMINHO TEXTO, resolver NOME, sair.\r\n");
}

fn mutate(
    operation: impl FnOnce(&mut Vfs<PROCESS_VFS_CAPACITY>) -> Result<(), carlio_os::vfs::VfsError>,
) {
    let result = (|| {
        let transaction = VfsMutationGuard::acquire()?;
        if transaction.before.is_none() {
            return Err(carlio_os::linux_abi::EIO);
        }
        operation(&mut PROCESS_VFS.lock()).map_err(vfs_errno)?;
        commit_vfs_mutation(&transaction)
    })();
    match result {
        Ok(()) => write("VFS: alteração gravada.\r\n"),
        Err(errno) => show_vfs_error(errno),
    }
}

fn show_vfs_error(errno: u16) {
    write("VFS: operação recusada (errno ");
    write_u32(u32::from(errno));
    write(").\r\n");
}

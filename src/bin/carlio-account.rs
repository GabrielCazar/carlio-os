//! Offline account provisioning for a dedicated persistence image.
use std::{
    fs::{File, OpenOptions},
    io::{self, IsTerminal, Read, Seek, SeekFrom, Write},
};

use argon2::{Block, password_hash::SaltString};
use carlio_os::{
    block::{BlockDevice, BlockError, SECTOR_SIZE},
    storage_reserve::StorageReserve,
    userland::{
        accounts::{
            ACCOUNT_GROUP_LIMIT, ACCOUNT_PATH, AccountDatabase, AccountError, DATABASE_BYTES,
            LOGIN_MEMORY_KIB, RESERVED_TEST_ACCOUNT,
        },
        kdf::derive_key_into,
        login::PASSWORD_LIMIT,
    },
    vfs::{AccessContext, Credentials, Vfs, VfsError},
    vfs_store::VfsStore,
};
use zeroize::Zeroizing;

type Overlay = Vfs<40>;
static OVERLAY: std::sync::Mutex<Overlay> = std::sync::Mutex::new(Overlay::new());
const MEMORY_KIB: usize = LOGIN_MEMORY_KIB;
const PASSES: u32 = 2;

struct Image(File, u64);

impl BlockDevice for Image {
    fn sector_count(&self) -> u64 {
        self.1
    }

    fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_SIZE]) -> Result<(), BlockError> {
        self.0
            .seek(SeekFrom::Start(lba * SECTOR_SIZE as u64))
            .and_then(|_| self.0.read_exact(out))
            .map_err(|_| BlockError::Device)
    }

    fn write_sector(&mut self, lba: u64, data: &[u8; SECTOR_SIZE]) -> Result<(), BlockError> {
        self.0
            .seek(SeekFrom::Start(lba * SECTOR_SIZE as u64))
            .and_then(|_| self.0.write_all(data))
            .map_err(|_| BlockError::Device)
    }

    fn flush(&mut self) -> Result<(), BlockError> {
        self.0.sync_all().map_err(|_| BlockError::Device)
    }
}

fn provision() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [path, name, uid, gid, groups @ ..] = args.as_slice() else {
        return Err(
            "Uso: carlio-account IMAGEM USUÁRIO UID GID [GID_SUPLEMENTAR ...] < senha".into(),
        );
    };
    if name == RESERVED_TEST_ACCOUNT {
        return Err("Nome reservado ao teste de boot.".into());
    }
    let parse_id = |id: &str| {
        id.parse::<u32>()
            .ok()
            .filter(|&id| id != 0 && id != u32::MAX)
            .ok_or("UID e GID devem ser identificadores de usuário sem privilégios.")
    };
    let credentials = Credentials {
        uid: parse_id(uid)?,
        gid: parse_id(gid)?,
    };
    let mut supplementary = Vec::new();
    for group in groups {
        let group = parse_id(group)?;
        if supplementary.len() == ACCOUNT_GROUP_LIMIT
            || group == credentials.gid
            || supplementary.contains(&group)
        {
            return Err(
                "Informe até oito grupos suplementares distintos do grupo primário.".into(),
            );
        }
        supplementary.push(group);
    }
    let root = Credentials { uid: 0, gid: 0 };
    let root_context = AccessContext::new(root, &[], 0);
    let error = |error: &dyn std::fmt::Debug| format!("{error:?}");

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| error(&e))?;
    let metadata = file.metadata().map_err(|e| error(&e))?;
    if !metadata.is_file() || metadata.len() % SECTOR_SIZE as u64 != 0 {
        return Err("A imagem precisa ser um arquivo regular alinhado a setores.".into());
    }
    file.try_lock().map_err(|e| error(&e))?;
    let mut image = Image(file, metadata.len() / SECTOR_SIZE as u64);
    let reserve = StorageReserve::detect(&mut image)
        .map_err(|e| error(&e))?
        .ok_or("Imagem sem reserva dedicada do CarlioOS.")?;
    let region = reserve.vfs(image).map_err(|e| error(&e))?;
    let mut snapshot = vec![0; Overlay::SNAPSHOT_MAX];
    let (mut store, len) = VfsStore::open(region, &mut snapshot).map_err(|e| error(&e))?;
    if store.recovery_only() {
        return Err("Overlay degradado. Preserve a imagem antes de recuperar as contas.".into());
    }
    let mut vfs = OVERLAY.lock().map_err(|e| error(&e))?;
    if len == 0 {
        vfs.create_fhs_layout().map_err(|e| error(&e))?;
    } else {
        vfs.restore_snapshot(&snapshot[..len])
            .map_err(|e| error(&e))?;
    }

    let mut database = match AccountDatabase::load_protected(&vfs) {
        Ok(database) => database,
        Err(AccountError::Vfs(VfsError::NotFound)) => {
            vfs.create_file("/etc", "carlio-accounts.db", root, 0o600)
                .map_err(|e| error(&e))?;
            AccountDatabase::new()
        }
        Err(e) => return Err(error(&e)),
    };
    if io::stdin().is_terminal() {
        return Err("Forneça a senha por um pipe com a entrada do terminal sem eco.".into());
    }
    let mut password = Zeroizing::new([0; PASSWORD_LIMIT + 2]);
    let mut input = io::stdin().lock();
    let mut length = 0;
    while length < password.len() {
        let count = input.read(&mut password[length..]).map_err(|e| error(&e))?;
        if count == 0 {
            break;
        }
        length += count;
    }
    if password[..length].last() == Some(&b'\n') {
        length -= 1;
    }
    if !(12..=PASSWORD_LIMIT).contains(&length)
        || password[..length]
            .iter()
            .any(|&byte| byte < 32 || byte == 127)
    {
        return Err("Informe uma senha de 12 a 1024 bytes sem caracteres de controle.".into());
    }
    let mut salt = [0; 16];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut salt))
        .map_err(|e| error(&e))?;
    let mut memory = vec![Block::new(); MEMORY_KIB];
    let mut output = [0; 32];
    derive_key_into(
        &password[..length],
        &salt,
        PASSES,
        1,
        &mut memory,
        &mut output,
    )
    .map_err(|e| error(&e))?;
    let salt = SaltString::encode_b64(&salt).map_err(|e| error(&e))?;
    let output = argon2::password_hash::Output::new(&output).map_err(|e| error(&e))?;
    let hash = format!("$argon2id$v=19$m={MEMORY_KIB},t={PASSES},p=1${salt}${output}");
    database
        .add(name, credentials, &supplementary, hash.as_bytes())
        .map_err(|e| error(&e))?;
    let mut bytes = [0; DATABASE_BYTES];
    let len = database.encode(&mut bytes).map_err(|e| error(&e))?;
    vfs.write_file_as(ACCOUNT_PATH, root_context, &bytes[..len])
        .map_err(|e| error(&e))?;
    AccountDatabase::load_protected(&vfs).map_err(|e| error(&e))?;
    let len = vfs.encode_snapshot(&mut snapshot).map_err(|e| error(&e))?;
    store.commit(&snapshot[..len]).map_err(|e| error(&e))?;
    Ok(())
}

fn main() -> std::process::ExitCode {
    match provision() {
        Ok(()) => {
            println!("Conta gravada no overlay protegido.");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

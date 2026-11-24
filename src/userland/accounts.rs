//! Banco serializável de contas para o host e futuros backends persistentes.
//!
//! Armazena somente hashes opacos. A verificação da senha é delegada a uma
//! implementação criptográfica externa; esta camada não inventa KDF próprio.

use super::login::CredentialVerifier;
use crate::vfs::{AccessContext, Credentials, NodeKind, Vfs, VfsError};

pub const ACCOUNT_LIMIT: usize = 16;
pub const ACCOUNT_NAME_BYTES: usize = 32;
pub const PASSWORD_HASH_BYTES: usize = 128;
pub const ACCOUNT_GROUP_LIMIT: usize = 8;
const MAGIC: &[u8; 8] = b"CARLAC01";
const HEADER_BYTES: usize = 16;
const RECORD_BYTES: usize =
    ACCOUNT_NAME_BYTES + 8 + 2 + PASSWORD_HASH_BYTES + ACCOUNT_GROUP_LIMIT * 4;
pub const DATABASE_BYTES: usize = HEADER_BYTES + ACCOUNT_LIMIT * RECORD_BYTES;
pub const ACCOUNT_PATH: &str = "/etc/carlio-accounts.db";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountError {
    Full,
    InvalidName,
    InvalidHash,
    Duplicate,
    Corrupt,
    BufferTooSmall,
    InvalidSize,
    UnsafePermissions,
    Vfs(VfsError),
    Storage(crate::hermione::FsError),
}

#[derive(Clone, Copy)]
struct Account {
    name: [u8; ACCOUNT_NAME_BYTES],
    credentials: Credentials,
    hash: [u8; PASSWORD_HASH_BYTES],
    hash_len: u8,
    groups: [u32; ACCOUNT_GROUP_LIMIT],
    group_count: u8,
    occupied: bool,
}

impl Account {
    const EMPTY: Self = Self {
        name: [0; ACCOUNT_NAME_BYTES],
        credentials: Credentials { uid: 0, gid: 0 },
        hash: [0; PASSWORD_HASH_BYTES],
        hash_len: 0,
        groups: [0; ACCOUNT_GROUP_LIMIT],
        group_count: 0,
        occupied: false,
    };
    fn username(&self) -> &str {
        let length = self
            .name
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(self.name.len());
        core::str::from_utf8(&self.name[..length]).unwrap_or("")
    }
}

pub struct AccountDatabase {
    entries: [Account; ACCOUNT_LIMIT],
    count: usize,
}
impl AccountDatabase {
    /// Loads the account database from the VFS overlay. The caller must open
    /// the persistent overlay before using this database for system login.
    pub fn load_protected<const N: usize>(vfs: &Vfs<N>) -> Result<Self, AccountError> {
        let root = AccessContext::new(Credentials { uid: 0, gid: 0 }, &[], 0);
        for path in ["/", "/etc"] {
            let directory = vfs.metadata(path, root).map_err(AccountError::Vfs)?;
            if directory.kind != NodeKind::Directory
                || directory.uid != 0
                || directory.mode & 0o022 != 0
            {
                return Err(AccountError::UnsafePermissions);
            }
        }
        let file = vfs
            .metadata(ACCOUNT_PATH, root)
            .map_err(AccountError::Vfs)?;
        if file.kind != NodeKind::File || file.uid != 0 || file.gid != 0 || file.mode != 0o600 {
            return Err(AccountError::UnsafePermissions);
        }
        if file.size != DATABASE_BYTES as u64 {
            return Err(AccountError::InvalidSize);
        }
        let mut bytes = [0; DATABASE_BYTES];
        let length = vfs
            .read_file_as(ACCOUNT_PATH, root, &mut bytes)
            .map_err(AccountError::Vfs)?;
        Self::decode(&bytes[..length])
    }

    pub const fn new() -> Self {
        Self {
            entries: [Account::EMPTY; ACCOUNT_LIMIT],
            count: 0,
        }
    }

    /// `password_hash` deve ser um hash forte (por exemplo PHC Argon2id) já
    /// produzido pelo backend de credenciais; senhas em claro são recusadas.
    pub fn add(
        &mut self,
        username: &str,
        credentials: Credentials,
        groups: &[u32],
        password_hash: &[u8],
    ) -> Result<(), AccountError> {
        if username.is_empty()
            || username.len() >= ACCOUNT_NAME_BYTES
            || !username
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(AccountError::InvalidName);
        }
        if password_hash.is_empty()
            || password_hash.len() > PASSWORD_HASH_BYTES
            || !password_hash.starts_with(b"$")
            || password_hash.contains(&0)
        {
            return Err(AccountError::InvalidHash);
        }
        if groups.len() > ACCOUNT_GROUP_LIMIT {
            return Err(AccountError::InvalidHash);
        }
        if self.entries[..self.count]
            .iter()
            .any(|entry| entry.username() == username)
        {
            return Err(AccountError::Duplicate);
        }
        if self.count == ACCOUNT_LIMIT {
            return Err(AccountError::Full);
        }
        let mut entry = Account::EMPTY;
        entry.name[..username.len()].copy_from_slice(username.as_bytes());
        entry.credentials = credentials;
        entry.hash[..password_hash.len()].copy_from_slice(password_hash);
        entry.hash_len = password_hash.len() as u8;
        entry.groups[..groups.len()].copy_from_slice(groups);
        entry.group_count = groups.len() as u8;
        entry.occupied = true;
        self.entries[self.count] = entry;
        self.count += 1;
        Ok(())
    }

    pub fn encode(&self, output: &mut [u8]) -> Result<usize, AccountError> {
        if output.len() < DATABASE_BYTES {
            return Err(AccountError::BufferTooSmall);
        }
        let bytes = &mut output[..DATABASE_BYTES];
        bytes.fill(0);
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..10].copy_from_slice(&1u16.to_le_bytes());
        bytes[10..12].copy_from_slice(&(self.count as u16).to_le_bytes());
        for (index, entry) in self.entries[..self.count].iter().enumerate() {
            let start = HEADER_BYTES + index * RECORD_BYTES;
            let record = &mut bytes[start..start + RECORD_BYTES];
            record[..ACCOUNT_NAME_BYTES].copy_from_slice(&entry.name);
            record[32..36].copy_from_slice(&entry.credentials.uid.to_le_bytes());
            record[36..40].copy_from_slice(&entry.credentials.gid.to_le_bytes());
            record[40] = entry.hash_len;
            record[41] = entry.group_count;
            record[42..42 + PASSWORD_HASH_BYTES].copy_from_slice(&entry.hash);
            let groups_start = 42 + PASSWORD_HASH_BYTES;
            for (group_index, group) in entry.groups.iter().enumerate() {
                let offset = groups_start + group_index * 4;
                record[offset..offset + 4].copy_from_slice(&group.to_le_bytes());
            }
        }
        let checksum = crc32(bytes);
        bytes[12..16].copy_from_slice(&checksum.to_le_bytes());
        Ok(DATABASE_BYTES)
    }

    /// Persists the opaque account records as one Hermione file. The volume
    /// supplies journaling, checksums and flush ordering; callers must protect
    /// the containing device against offline disclosure.
    pub fn persist<D: crate::hermione::BlockDevice>(
        &self,
        volume: &mut crate::hermione::HermioneVolume<D>,
        name: &str,
    ) -> Result<(), AccountError> {
        let mut bytes = [0u8; DATABASE_BYTES];
        let length = self.encode(&mut bytes)?;
        volume
            .write_file(name, &bytes[..length])
            .map_err(AccountError::Storage)
    }

    pub fn load<D: crate::hermione::BlockDevice>(
        volume: &mut crate::hermione::HermioneVolume<D>,
        name: &str,
    ) -> Result<Self, AccountError> {
        let mut bytes = [0u8; DATABASE_BYTES];
        let length = volume
            .read_file(name, &mut bytes)
            .map_err(AccountError::Storage)?;
        if length != DATABASE_BYTES {
            return Err(AccountError::InvalidSize);
        }
        Self::decode(&bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, AccountError> {
        if bytes.len() != DATABASE_BYTES
            || &bytes[..8] != MAGIC
            || u16::from_le_bytes([bytes[8], bytes[9]]) != 1
        {
            return Err(AccountError::Corrupt);
        }
        let count = u16::from_le_bytes([bytes[10], bytes[11]]) as usize;
        if count > ACCOUNT_LIMIT {
            return Err(AccountError::Corrupt);
        }
        let stored = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
        let mut check = [0u8; DATABASE_BYTES];
        check.copy_from_slice(bytes);
        check[12..16].fill(0);
        if crc32(&check) != stored {
            return Err(AccountError::Corrupt);
        }
        let mut database = Self::new();
        for index in 0..count {
            let start = HEADER_BYTES + index * RECORD_BYTES;
            let record = &bytes[start..start + RECORD_BYTES];
            let name_end = record[..ACCOUNT_NAME_BYTES]
                .iter()
                .position(|byte| *byte == 0)
                .ok_or(AccountError::Corrupt)?;
            let name =
                core::str::from_utf8(&record[..name_end]).map_err(|_| AccountError::Corrupt)?;
            let hash_len = record[40] as usize;
            let group_count = record[41] as usize;
            if hash_len == 0 || hash_len > PASSWORD_HASH_BYTES || group_count > ACCOUNT_GROUP_LIMIT
            {
                return Err(AccountError::Corrupt);
            }
            let uid = u32::from_le_bytes(record[32..36].try_into().unwrap());
            let gid = u32::from_le_bytes(record[36..40].try_into().unwrap());
            let groups_start = 42 + PASSWORD_HASH_BYTES;
            let mut groups = [0u32; ACCOUNT_GROUP_LIMIT];
            for (slot, group) in groups.iter_mut().take(group_count).enumerate() {
                let offset = groups_start + slot * 4;
                *group = u32::from_le_bytes(record[offset..offset + 4].try_into().unwrap());
            }
            let hash_start = 42;
            database
                .add(
                    name,
                    Credentials { uid, gid },
                    &groups[..group_count],
                    &record[hash_start..hash_start + hash_len],
                )
                .map_err(|_| AccountError::Corrupt)?;
        }
        Ok(database)
    }

    fn find(&self, username: &str) -> Option<&Account> {
        self.entries[..self.count]
            .iter()
            .find(|entry| entry.username() == username)
    }
}

impl Default for AccountDatabase {
    fn default() -> Self {
        Self::new()
    }
}

/// Adapta a base para login; o backend implementa Argon2id/scrypt verificado.
pub struct AccountVerifier<'a, H> {
    database: &'a AccountDatabase,
    hash_verifier: H,
}
impl<'a, H> AccountVerifier<'a, H> {
    pub const fn new(database: &'a AccountDatabase, hash_verifier: H) -> Self {
        Self {
            database,
            hash_verifier,
        }
    }
}

pub trait PasswordHashVerifier {
    fn verify(&mut self, encoded_hash: &[u8], password: &[u8]) -> bool;
}
impl<H: PasswordHashVerifier> CredentialVerifier for AccountVerifier<'_, H> {
    fn verify(&mut self, username: &str, password: &[u8]) -> Option<Credentials> {
        let account = self.database.find(username)?;
        self.hash_verifier
            .verify(&account.hash[..account.hash_len as usize], password)
            .then_some(account.credentials)
    }
    fn supplementary_groups(&self, username: &str) -> ([u32; 8], usize) {
        match self.database.find(username) {
            Some(account) => (account.groups, account.group_count as usize),
            None => ([0; 8], 0),
        }
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::userland::login::LoginManager;

    struct TestHash;
    impl PasswordHashVerifier for TestHash {
        fn verify(&mut self, hash: &[u8], password: &[u8]) -> bool {
            hash == b"$test$opaque" && password == b"correct"
        }
    }

    struct Disk(std::vec::Vec<[u8; crate::hermione::SECTOR_SIZE]>);
    impl crate::hermione::BlockDevice for Disk {
        fn sector_count(&self) -> u64 {
            self.0.len() as u64
        }
        fn read_sector(
            &mut self,
            lba: u64,
            out: &mut [u8; crate::hermione::SECTOR_SIZE],
        ) -> Result<(), crate::block::BlockError> {
            *out = *self
                .0
                .get(lba as usize)
                .ok_or(crate::block::BlockError::Device)?;
            Ok(())
        }
        fn write_sector(
            &mut self,
            lba: u64,
            data: &[u8; crate::hermione::SECTOR_SIZE],
        ) -> Result<(), crate::block::BlockError> {
            *self
                .0
                .get_mut(lba as usize)
                .ok_or(crate::block::BlockError::Device)? = *data;
            Ok(())
        }
        fn flush(&mut self) -> Result<(), crate::block::BlockError> {
            Ok(())
        }
    }

    fn volume() -> crate::hermione::HermioneVolume<Disk> {
        crate::hermione::HermioneVolume::format(Disk(
            std::vec![[0; crate::hermione::SECTOR_SIZE]; 128],
        ))
        .unwrap()
    }

    #[test]
    fn protected_accounts_require_safe_metadata_after_overlay_restore() {
        let root = Credentials { uid: 0, gid: 0 };
        let user = Credentials { uid: 42, gid: 42 };
        let context = AccessContext::new(root, &[], 0);
        let mut original = Vfs::<4>::new();
        original.create_dir("/", "etc", root, 0o755).unwrap();
        original
            .create_file("/etc", "carlio-accounts.db", root, 0o600)
            .unwrap();
        let mut accounts = AccountDatabase::new();
        accounts
            .add("service", user, &[8], b"$test$opaque")
            .unwrap();
        let mut encoded = [0; DATABASE_BYTES];
        accounts.encode(&mut encoded).unwrap();
        original
            .write_file_as(ACCOUNT_PATH, context, &encoded)
            .unwrap();

        let mut snapshot = std::vec![0; Vfs::<4>::SNAPSHOT_MAX];
        let length = original.encode_snapshot(&mut snapshot).unwrap();
        let mut restored = Vfs::<4>::new();
        restored.restore_overlay(&snapshot[..length]).unwrap();
        assert_eq!(
            restored.read_file_as(ACCOUNT_PATH, AccessContext::new(user, &[], 0), &mut encoded),
            Err(VfsError::PermissionDenied)
        );

        let accounts = AccountDatabase::load_protected(&restored).unwrap();
        let mut login = LoginManager::new(AccountVerifier::new(&accounts, TestHash));
        assert!(login.authenticate("service", b"wrong").is_err());
        let session = login.authenticate("service", b"correct").unwrap();
        assert_eq!(session.credentials(), user);
        assert_eq!(session.supplementary_groups(), &[8]);

        restored.set_mode(ACCOUNT_PATH, root, 0o644).unwrap();
        assert!(matches!(
            AccountDatabase::load_protected(&restored),
            Err(AccountError::UnsafePermissions)
        ));
        restored.set_mode(ACCOUNT_PATH, root, 0o600).unwrap();
        restored.set_mode("/etc", root, 0o777).unwrap();
        assert!(matches!(
            AccountDatabase::load_protected(&restored),
            Err(AccountError::UnsafePermissions)
        ));
    }

    #[test]
    fn account_database_persists_through_hermione_remount() {
        let mut database = AccountDatabase::new();
        database
            .add(
                "service",
                Credentials { uid: 42, gid: 42 },
                &[8],
                b"$test$opaque",
            )
            .unwrap();
        let mut volume = volume();
        database.persist(&mut volume, "accounts.db").unwrap();
        let disk = volume.into_device();
        let mut remounted = crate::hermione::HermioneVolume::mount(disk).unwrap();
        let reopened = AccountDatabase::load(&mut remounted, "accounts.db").unwrap();
        let mut login = LoginManager::new(AccountVerifier::new(&reopened, TestHash));
        assert_eq!(
            login
                .authenticate("service", b"correct")
                .unwrap()
                .credentials(),
            Credentials { uid: 42, gid: 42 }
        );
    }

    #[test]
    fn account_records_roundtrip_and_authenticate_without_plaintext_storage() {
        let mut database = AccountDatabase::new();
        database
            .add(
                "alice",
                Credentials {
                    uid: 1000,
                    gid: 100,
                },
                &[7, 9],
                b"$test$opaque",
            )
            .unwrap();
        let mut image = [0u8; DATABASE_BYTES];
        let length = database.encode(&mut image).unwrap();
        assert!(
            !image[..length]
                .windows(7)
                .any(|window| window == b"correct")
        );
        let reopened = AccountDatabase::decode(&image[..length]).unwrap();
        let mut login = LoginManager::new(AccountVerifier::new(&reopened, TestHash));
        let session = login.authenticate("alice", b"correct").unwrap();
        assert_eq!(
            session.credentials(),
            Credentials {
                uid: 1000,
                gid: 100
            }
        );
        assert_eq!(session.supplementary_groups(), &[7, 9]);
        assert!(login.authenticate("alice", b"wrong").is_err());
    }

    #[test]
    fn account_database_rejects_plaintext_duplicate_and_corruption() {
        let mut database = AccountDatabase::new();
        assert_eq!(
            database.add("alice", Credentials { uid: 1, gid: 1 }, &[], b"plaintext"),
            Err(AccountError::InvalidHash)
        );
        database
            .add("alice", Credentials { uid: 1, gid: 1 }, &[], b"$test$hash")
            .unwrap();
        assert_eq!(
            database.add("alice", Credentials { uid: 2, gid: 2 }, &[], b"$test$hash"),
            Err(AccountError::Duplicate)
        );
        let mut image = [0u8; DATABASE_BYTES];
        database.encode(&mut image).unwrap();
        image[40] ^= 1;
        assert!(matches!(
            AccountDatabase::decode(&image),
            Err(AccountError::Corrupt)
        ));
    }
}

//! Volume Hermione experimental sobre dispositivos de setores de 512 bytes.
//!
//! Formato experimental sem cifra. Dados possuem duas cópias checksumadas e a
//! publicação da nova geração usa um journal redo de um setor.

pub use crate::block::{BlockDevice, SECTOR_SIZE};
const MAGIC: &[u8; 8] = b"HERMFS03";
const DIRECTORY_MAGIC: &[u8; 8] = b"HERM-DIR";
const VERSION: u32 = 3;
const DIRECTORY_LBAS: [u64; 2] = [1, 2];
const JOURNAL_LBA: u64 = 3;
const DIRECTORY_HEADER_SIZE: usize = 24;
const DATA_START: u64 = 4;
const ENTRY_SIZE: usize = 44;
const ENTRY_COUNT: usize = (SECTOR_SIZE - DIRECTORY_HEADER_SIZE) / ENTRY_SIZE;
const NAME_SIZE: usize = 24;
pub const MAX_FILES: usize = ENTRY_COUNT;
/// O maior volume que endereça LBAs de 0 a 2^32-1 (2 TiB em setores de 512 B).
pub const MAX_VOLUME_SECTORS: u64 = u32::MAX as u64 + 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsError {
    Device,
    TooSmall,
    InvalidSuperblock,
    CorruptDirectory,
    InvalidName,
    FileNotFound,
    DirectoryFull,
    NoSpace,
    BufferTooSmall,
    FileTooLarge,
    ChecksumMismatch,
}

impl From<crate::block::BlockError> for FsError {
    fn from(_: crate::block::BlockError) -> Self {
        Self::Device
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileInfo {
    pub name: [u8; NAME_SIZE],
    pub size: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScrubReport {
    pub files_verified: usize,
    pub copies_repaired: usize,
}

#[derive(Clone, Copy)]
struct Entry {
    name: [u8; NAME_SIZE],
    start: u32,
    mirror_start: u32,
    sectors: u32,
    size: u32,
    checksum: u32,
}

impl Entry {
    const EMPTY: Self = Self {
        name: [0; NAME_SIZE],
        start: 0,
        mirror_start: 0,
        sectors: 0,
        size: 0,
        checksum: 0,
    };
    fn occupied(&self) -> bool {
        self.name[0] != 0
    }
}

pub struct HermioneVolume<D> {
    device: D,
    sectors: u64,
    active_directory: u64,
    generation: u64,
}

impl<D: BlockDevice> HermioneVolume<D> {
    pub fn format(mut device: D) -> Result<Self, FsError> {
        let sectors = device.sector_count();
        if sectors <= DATA_START + 1 {
            return Err(FsError::TooSmall);
        }
        if sectors > MAX_VOLUME_SECTORS {
            return Err(FsError::FileTooLarge);
        }
        let mut superblock = [0; SECTOR_SIZE];
        superblock[..8].copy_from_slice(MAGIC);
        put_u32(&mut superblock, 8, VERSION);
        put_u64(&mut superblock, 16, sectors);
        let checksum = crc_with_zeroed_field(&mut superblock, 12);
        put_u32(&mut superblock, 12, checksum);
        device.write_sector(0, &superblock)?;
        let mut volume = Self {
            device,
            sectors,
            active_directory: DIRECTORY_LBAS[0],
            generation: 0,
        };
        let entries = [Entry::EMPTY; ENTRY_COUNT];
        let sector = encode_directory(&entries, 0);
        volume.device.write_sector(DIRECTORY_LBAS[0], &sector)?;
        volume.device.write_sector(DIRECTORY_LBAS[1], &sector)?;
        volume.device.write_sector(JOURNAL_LBA, &[0; SECTOR_SIZE])?;
        volume.device.flush()?;
        Ok(volume)
    }

    pub fn mount(mut device: D) -> Result<Self, FsError> {
        let device_sectors = device.sector_count();
        if device_sectors <= DATA_START + 1 {
            return Err(FsError::TooSmall);
        }
        let mut sector = [0; SECTOR_SIZE];
        device.read_sector(0, &mut sector)?;
        let stored_sectors = get_u64(&sector, 16);
        if &sector[..8] != MAGIC
            || get_u32(&sector, 8) != VERSION
            || crc_with_zeroed_field(&mut sector, 12) != get_u32(&sector, 12)
            || stored_sectors < DATA_START + 1
            || stored_sectors > device_sectors
            || stored_sectors > MAX_VOLUME_SECTORS
        {
            return Err(FsError::InvalidSuperblock);
        }
        let mut volume = Self {
            sectors: stored_sectors,
            device,
            active_directory: DIRECTORY_LBAS[0],
            generation: 0,
        };
        volume.read_directory()?;
        if volume.active_directory == JOURNAL_LBA {
            volume.recover_journal()?;
        }
        Ok(volume)
    }

    pub fn into_device(self) -> D {
        self.device
    }

    pub fn list(&mut self, output: &mut [Option<FileInfo>; ENTRY_COUNT]) -> Result<usize, FsError> {
        let entries = self.read_directory()?;
        let mut count = 0;
        for (slot, entry) in output.iter_mut().zip(entries.iter()) {
            *slot = if entry.occupied() {
                Some(FileInfo {
                    name: entry.name,
                    size: entry.size,
                })
            } else {
                None
            };
            if entry.occupied() {
                count += 1;
            }
        }
        Ok(count)
    }

    pub fn write_file(&mut self, name: &str, data: &[u8]) -> Result<(), FsError> {
        let encoded = encode_name(name)?;
        let mut entries = self.read_directory()?;
        let sectors_needed = data.len().div_ceil(SECTOR_SIZE);
        if sectors_needed > u32::MAX as usize || data.len() > u32::MAX as usize {
            return Err(FsError::FileTooLarge);
        }
        let index = entries
            .iter()
            .position(|e| e.occupied() && e.name == encoded)
            .or_else(|| entries.iter().position(|e| !e.occupied()))
            .ok_or(FsError::DirectoryFull)?;
        let sectors = sectors_needed.max(1) as u32;
        let start = self
            .find_extent(&entries, sectors)
            .ok_or(FsError::NoSpace)?;
        entries[index] = Entry {
            name: encoded,
            start,
            mirror_start: 0,
            sectors,
            size: data.len() as u32,
            checksum: 0,
        };
        let mirror_start = self
            .find_extent(&entries, sectors)
            .ok_or(FsError::NoSpace)?;
        let mut checksum = !0u32;
        for n in 0..sectors_needed {
            let mut sector = [0; SECTOR_SIZE];
            let from = n * SECTOR_SIZE;
            let to = core::cmp::min(from + SECTOR_SIZE, data.len());
            sector[..to - from].copy_from_slice(&data[from..to]);
            checksum = crc32_update(checksum, &sector[..to - from]);
            self.device.write_sector(start as u64 + n as u64, &sector)?;
            self.device
                .write_sector(mirror_start as u64 + n as u64, &sector)?;
        }
        // Dados novos devem chegar à mídia antes da publicação do diretório.
        self.device.flush()?;
        entries[index].mirror_start = mirror_start;
        entries[index].checksum = !checksum;
        self.write_directory(&entries)
    }

    pub fn read_file(&mut self, name: &str, output: &mut [u8]) -> Result<usize, FsError> {
        self.read_file_with_progress(name, output, |_| {})
    }

    /// Variante com observação de fases para diagnóstico de dispositivos físicos.
    /// 1=metadados, 2=integridade/reparo, 3+n=setor de dados concluído.
    pub fn read_file_with_progress(
        &mut self,
        name: &str,
        output: &mut [u8],
        mut progress: impl FnMut(usize),
    ) -> Result<usize, FsError> {
        let encoded = encode_name(name)?;
        let entries = self.read_directory()?;
        progress(1);
        let entry = entries
            .iter()
            .find(|e| e.occupied() && e.name == encoded)
            .copied()
            .ok_or(FsError::FileNotFound)?;
        if output.len() < entry.size as usize {
            return Err(FsError::BufferTooSmall);
        }
        self.repair_entry(&entry)?;
        progress(2);
        let mut remaining = entry.size as usize;
        let mut offset = 0;
        for n in 0..entry.sectors {
            let mut sector = [0; SECTOR_SIZE];
            if let Err(error) = self
                .device
                .read_sector(entry.start as u64 + n as u64, &mut sector)
            {
                output[..entry.size as usize].fill(0);
                return Err(FsError::from(error));
            }
            let used = core::cmp::min(remaining, SECTOR_SIZE);
            output[offset..offset + used].copy_from_slice(&sector[..used]);
            offset += used;
            remaining -= used;
            progress(3 + n as usize);
        }
        Ok(entry.size as usize)
    }

    pub fn delete_file(&mut self, name: &str) -> Result<(), FsError> {
        let encoded = encode_name(name)?;
        let mut entries = self.read_directory()?;
        let entry = entries
            .iter_mut()
            .find(|e| e.occupied() && e.name == encoded)
            .ok_or(FsError::FileNotFound)?;
        *entry = Entry::EMPTY;
        self.write_directory(&entries)
    }

    /// Verifica todas as cópias e repara uma cópia usando somente a redundante íntegra.
    pub fn scrub(&mut self) -> Result<ScrubReport, FsError> {
        let entries = self.read_directory()?;
        let mut report = ScrubReport::default();
        for entry in entries.iter().filter(|entry| entry.occupied()) {
            report.copies_repaired += usize::from(self.repair_entry(entry)?);
            report.files_verified += 1;
        }
        Ok(report)
    }

    fn extent_checksum(&mut self, start: u32, sectors: u32, size: u32) -> Result<u32, FsError> {
        let mut remaining = size as usize;
        let mut checksum = !0u32;
        for n in 0..sectors {
            let mut sector = [0; SECTOR_SIZE];
            self.device
                .read_sector(start as u64 + n as u64, &mut sector)?;
            let used = core::cmp::min(remaining, SECTOR_SIZE);
            checksum = crc32_update(checksum, &sector[..used]);
            remaining -= used;
        }
        Ok(!checksum)
    }

    fn repair_entry(&mut self, entry: &Entry) -> Result<bool, FsError> {
        let primary = self.extent_checksum(entry.start, entry.sectors, entry.size);
        let mirror = self.extent_checksum(entry.mirror_start, entry.sectors, entry.size);
        let primary_ok = primary == Ok(entry.checksum);
        let mirror_ok = mirror == Ok(entry.checksum);
        let (source, target) = match (primary_ok, mirror_ok) {
            (true, true) => return Ok(false),
            (true, false) => (entry.start, entry.mirror_start),
            (false, true) => (entry.mirror_start, entry.start),
            (false, false) => {
                return Err(
                    if primary == Err(FsError::Device) && mirror == Err(FsError::Device) {
                        FsError::Device
                    } else {
                        FsError::ChecksumMismatch
                    },
                );
            }
        };
        for n in 0..entry.sectors {
            let mut sector = [0; SECTOR_SIZE];
            self.device
                .read_sector(source as u64 + n as u64, &mut sector)?;
            self.device
                .write_sector(target as u64 + n as u64, &sector)?;
        }
        self.device.flush()?;
        if self.extent_checksum(target, entry.sectors, entry.size)? != entry.checksum {
            return Err(FsError::ChecksumMismatch);
        }
        Ok(true)
    }

    fn find_extent(&self, entries: &[Entry; ENTRY_COUNT], count: u32) -> Option<u32> {
        let mut cursor = DATA_START;
        for _ in 0..=ENTRY_COUNT * 2 {
            let next = entries
                .iter()
                .filter(|e| e.occupied())
                .flat_map(|e| [e.start as u64, e.mirror_start as u64])
                .filter(|start| *start >= cursor)
                .min();
            if let Some(start) = next {
                if start.saturating_sub(cursor) >= count as u64 {
                    return Some(cursor as u32);
                }
                let extent_end = entries
                    .iter()
                    .filter(|e| e.occupied())
                    .flat_map(|e| [e.start as u64, e.mirror_start as u64])
                    .filter(|extent_start| *extent_start == start)
                    .map(|extent_start| {
                        extent_start
                            + entries
                                .iter()
                                .find(|e| {
                                    e.occupied()
                                        && (e.start as u64 == extent_start
                                            || e.mirror_start as u64 == extent_start)
                                })
                                .map(|e| e.sectors as u64)
                                .unwrap_or(0)
                    })
                    .max()?;
                cursor = extent_end;
            } else {
                if self.sectors.saturating_sub(cursor) >= count as u64 {
                    return Some(cursor as u32);
                }
                return None;
            }
        }
        None
    }

    fn read_directory(&mut self) -> Result<[Entry; ENTRY_COUNT], FsError> {
        let mut candidates = [None, None, None];
        for (index, lba) in DIRECTORY_LBAS.iter().enumerate() {
            let mut sector = [0; SECTOR_SIZE];
            self.device.read_sector(*lba, &mut sector)?;
            candidates[index] = parse_directory(&sector, self.sectors);
        }
        let mut journal = [0; SECTOR_SIZE];
        self.device.read_sector(JOURNAL_LBA, &mut journal)?;
        candidates[2] = parse_directory(&journal, self.sectors);
        let lbas = [DIRECTORY_LBAS[0], DIRECTORY_LBAS[1], JOURNAL_LBA];
        let mut selected: Option<(u64, Directory)> = None;
        for (index, candidate) in candidates.into_iter().enumerate() {
            if let Some(candidate) = candidate {
                if selected
                    .map(|(_, current)| {
                        generation_is_newer(candidate.generation, current.generation)
                    })
                    .unwrap_or(true)
                {
                    selected = Some((lbas[index], candidate));
                }
            }
        }
        let selected = selected.ok_or(FsError::CorruptDirectory)?;
        self.active_directory = selected.0;
        self.generation = selected.1.generation;
        Ok(selected.1.entries)
    }

    fn write_directory(&mut self, entries: &[Entry; ENTRY_COUNT]) -> Result<(), FsError> {
        let target = if self.active_directory == DIRECTORY_LBAS[0] {
            DIRECTORY_LBAS[1]
        } else {
            DIRECTORY_LBAS[0]
        };
        let generation = self.generation.wrapping_add(1);
        let sector = encode_directory(entries, generation);
        self.device.write_sector(JOURNAL_LBA, &sector)?;
        self.device.flush()?;
        self.device.write_sector(target, &sector)?;
        self.device.flush()?;
        self.device.write_sector(JOURNAL_LBA, &[0; SECTOR_SIZE])?;
        self.device.flush()?;
        self.active_directory = target;
        self.generation = generation;
        Ok(())
    }

    fn recover_journal(&mut self) -> Result<(), FsError> {
        let mut journal = [0; SECTOR_SIZE];
        self.device.read_sector(JOURNAL_LBA, &mut journal)?;
        if parse_directory(&journal, self.sectors).is_none() {
            return Err(FsError::CorruptDirectory);
        }
        let target = DIRECTORY_LBAS[0];
        self.device.write_sector(target, &journal)?;
        self.device.flush()?;
        self.device.write_sector(JOURNAL_LBA, &[0; SECTOR_SIZE])?;
        self.device.flush()?;
        self.active_directory = target;
        Ok(())
    }
}

impl<D: BlockDevice> crate::vfs::ReadOnlyMountBackend for HermioneVolume<D> {
    fn filesystem_type(&self) -> crate::vfs::FilesystemType {
        crate::vfs::FilesystemType::Hermione
    }

    fn read_file(&mut self, path: &str, output: &mut [u8]) -> Result<usize, crate::vfs::VfsError> {
        let name = path.trim_start_matches('/');
        if name.is_empty() || name.contains('/') {
            return Err(crate::vfs::VfsError::NotFound);
        }
        HermioneVolume::read_file(self, name, output).map_err(|error| match error {
            FsError::FileNotFound => crate::vfs::VfsError::NotFound,
            FsError::BufferTooSmall => crate::vfs::VfsError::BufferTooSmall,
            FsError::InvalidName => crate::vfs::VfsError::InvalidPath,
            _ => crate::vfs::VfsError::InvalidPath,
        })
    }
}

#[derive(Clone, Copy)]
struct Directory {
    generation: u64,
    entries: [Entry; ENTRY_COUNT],
}

fn parse_directory(sector: &[u8; SECTOR_SIZE], volume_sectors: u64) -> Option<Directory> {
    let mut checked = *sector;
    let checksum = get_u32(&checked, 12);
    put_u32(&mut checked, 12, 0);
    if &checked[..8] != DIRECTORY_MAGIC
        || get_u32(&checked, 8) != VERSION
        || crc32(&checked) != checksum
    {
        return None;
    }
    let generation = get_u64(&checked, 16);
    let mut entries = [Entry::EMPTY; ENTRY_COUNT];
    for (index, entry) in entries.iter_mut().enumerate() {
        let p = DIRECTORY_HEADER_SIZE + index * ENTRY_SIZE;
        entry.name.copy_from_slice(&sector[p..p + NAME_SIZE]);
        entry.start = get_u32(sector, p + 24);
        entry.mirror_start = get_u32(sector, p + 28);
        entry.sectors = get_u32(sector, p + 32);
        entry.size = get_u32(sector, p + 36);
        entry.checksum = get_u32(sector, p + 40);
        if entry.occupied() {
            let end = entry.start as u64 + entry.sectors as u64;
            let mirror_end = entry.mirror_start as u64 + entry.sectors as u64;
            let expected_sectors = (entry.size as u64).div_ceil(SECTOR_SIZE as u64).max(1);
            let name_len = entry.name.iter().position(|byte| *byte == 0);
            if (entry.start as u64) < DATA_START
                || (entry.mirror_start as u64) < DATA_START
                || entry.start == entry.mirror_start
                || extents_overlap(
                    entry.start,
                    entry.sectors,
                    entry.mirror_start,
                    entry.sectors,
                )
                || entry.sectors == 0
                || entry.sectors as u64 != expected_sectors
                || end > volume_sectors
                || mirror_end > volume_sectors
                || entry.size as u64 > entry.sectors as u64 * SECTOR_SIZE as u64
                || !valid_encoded_name(&entry.name, name_len)
            {
                return None;
            }
        }
    }
    for i in 0..ENTRY_COUNT {
        if !entries[i].occupied() {
            continue;
        }
        for j in i + 1..ENTRY_COUNT {
            if !entries[j].occupied() {
                continue;
            }
            let a = entries[i];
            let b = entries[j];
            if a.name == b.name
                || extents_overlap(a.start, a.sectors, b.start, b.sectors)
                || extents_overlap(a.start, a.sectors, b.mirror_start, b.sectors)
                || extents_overlap(a.mirror_start, a.sectors, b.start, b.sectors)
                || extents_overlap(a.mirror_start, a.sectors, b.mirror_start, b.sectors)
            {
                return None;
            }
        }
    }
    Some(Directory {
        generation,
        entries,
    })
}

fn encode_directory(entries: &[Entry; ENTRY_COUNT], generation: u64) -> [u8; SECTOR_SIZE] {
    let mut sector = [0; SECTOR_SIZE];
    sector[..8].copy_from_slice(DIRECTORY_MAGIC);
    put_u32(&mut sector, 8, VERSION);
    put_u64(&mut sector, 16, generation);
    for (index, entry) in entries.iter().enumerate() {
        let p = DIRECTORY_HEADER_SIZE + index * ENTRY_SIZE;
        sector[p..p + NAME_SIZE].copy_from_slice(&entry.name);
        put_u32(&mut sector, p + 24, entry.start);
        put_u32(&mut sector, p + 28, entry.mirror_start);
        put_u32(&mut sector, p + 32, entry.sectors);
        put_u32(&mut sector, p + 36, entry.size);
        put_u32(&mut sector, p + 40, entry.checksum);
    }
    let checksum = crc32(&sector);
    put_u32(&mut sector, 12, checksum);
    sector
}

fn extents_overlap(a_start: u32, a_count: u32, b_start: u32, b_count: u32) -> bool {
    (a_start as u64) < b_start as u64 + b_count as u64
        && (b_start as u64) < a_start as u64 + a_count as u64
}

fn generation_is_newer(candidate: u64, current: u64) -> bool {
    candidate != current && candidate.wrapping_sub(current) < (1u64 << 63)
}

fn crc_with_zeroed_field(sector: &mut [u8; SECTOR_SIZE], offset: usize) -> u32 {
    let stored = get_u32(sector, offset);
    put_u32(sector, offset, 0);
    let checksum = crc32(sector);
    put_u32(sector, offset, stored);
    checksum
}

fn encode_name(name: &str) -> Result<[u8; NAME_SIZE], FsError> {
    if name.is_empty()
        || name.len() >= NAME_SIZE
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(FsError::InvalidName);
    }
    let mut encoded = [0; NAME_SIZE];
    encoded[..name.len()].copy_from_slice(name.as_bytes());
    Ok(encoded)
}

fn valid_encoded_name(name: &[u8; NAME_SIZE], terminator: Option<usize>) -> bool {
    let Some(len) = terminator else {
        return false;
    };
    len > 0
        && name[len..].iter().all(|byte| *byte == 0)
        && name[..len]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(byte))
}

fn get_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn get_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}
fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}
fn crc32(bytes: &[u8]) -> u32 {
    !crc32_update(!0, bytes)
}
fn crc32_update(mut crc: u32, bytes: &[u8]) -> u32 {
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Clone)]
    struct MemDisk {
        sectors: Vec<[u8; SECTOR_SIZE]>,
        reported_sectors: u64,
        writes: usize,
        tear_write_at: Option<usize>,
        flushes: usize,
        fail_flush_at: Option<usize>,
        flush_after_write: Vec<usize>,
    }
    impl BlockDevice for MemDisk {
        fn sector_count(&self) -> u64 {
            self.reported_sectors
        }
        fn read_sector(
            &mut self,
            lba: u64,
            out: &mut [u8; SECTOR_SIZE],
        ) -> Result<(), crate::block::BlockError> {
            *out = *self
                .sectors
                .get(lba as usize)
                .ok_or(crate::block::BlockError::Device)?;
            Ok(())
        }
        fn write_sector(
            &mut self,
            lba: u64,
            data: &[u8; SECTOR_SIZE],
        ) -> Result<(), crate::block::BlockError> {
            let index = self.writes;
            self.writes += 1;
            let target = self
                .sectors
                .get_mut(lba as usize)
                .ok_or(crate::block::BlockError::Device)?;
            if self.tear_write_at == Some(index) {
                target[..SECTOR_SIZE / 32].copy_from_slice(&data[..SECTOR_SIZE / 32]);
                return Err(crate::block::BlockError::Device);
            }
            *target = *data;
            Ok(())
        }
        fn flush(&mut self) -> Result<(), crate::block::BlockError> {
            let index = self.flushes;
            self.flushes += 1;
            self.flush_after_write.push(self.writes);
            if self.fail_flush_at == Some(index) {
                return Err(crate::block::BlockError::Device);
            }
            Ok(())
        }
    }
    fn disk() -> MemDisk {
        MemDisk {
            sectors: vec![[0; SECTOR_SIZE]; 128],
            reported_sectors: 128,
            writes: 0,
            tear_write_at: None,
            flushes: 0,
            fail_flush_at: None,
            flush_after_write: Vec::new(),
        }
    }

    #[test]
    fn vfs_root_snapshot_survives_herm_fs_remount() {
        let root =
            crate::vfs::AccessContext::new(crate::vfs::Credentials { uid: 0, gid: 0 }, &[], 0);
        let mut vfs = crate::vfs::Vfs::<8>::new();
        vfs.create_dir("/", "etc", root.credentials, 0o755).unwrap();
        vfs.create_file("/etc", "os-release", root.credentials, 0o644)
            .unwrap();
        vfs.write_file_as("/etc/os-release", root, b"NAME=CarlioOS\n")
            .unwrap();
        let mut snapshot = [0; 4096];
        let length = vfs.encode_snapshot(&mut snapshot).unwrap();

        let mut herm = HermioneVolume::format(disk()).unwrap();
        herm.write_file("rootfs.img", &snapshot[..length]).unwrap();
        let device = herm.into_device();
        let mut remounted = HermioneVolume::mount(device).unwrap();
        let mut restored_bytes = [0; 4096];
        let restored_len = remounted
            .read_file("rootfs.img", &mut restored_bytes)
            .unwrap();
        let restored =
            crate::vfs::Vfs::<8>::decode_snapshot(&restored_bytes[..restored_len]).unwrap();
        let mut contents = [0; 32];
        assert_eq!(
            restored.read_file_as("/etc/os-release", root, &mut contents),
            Ok(14)
        );
        assert_eq!(&contents[..14], b"NAME=CarlioOS\n");
    }

    #[test]
    fn supports_exact_two_tib_sector_count_without_u32_truncation() {
        let mut medium = disk();
        medium.reported_sectors = MAX_VOLUME_SECTORS;
        let formatted = HermioneVolume::format(medium).unwrap();
        let mounted = HermioneVolume::mount(formatted.into_device()).unwrap();
        assert_eq!(mounted.sectors, MAX_VOLUME_SECTORS);

        let mut too_large = disk();
        too_large.reported_sectors = MAX_VOLUME_SECTORS + 1;
        assert_eq!(
            HermioneVolume::format(too_large).err(),
            Some(FsError::FileTooLarge)
        );
    }

    #[test]
    fn format_write_read_and_mount_persist() {
        let mut fs = HermioneVolume::format(disk()).unwrap();
        fs.write_file("notas.txt", b"arquivo persistente").unwrap();
        let device = fs.into_device();
        let mut fs = HermioneVolume::mount(device).unwrap();
        let mut output = [0; 32];
        let count = fs.read_file("notas.txt", &mut output).unwrap();
        assert_eq!(&output[..count], b"arquivo persistente");
    }

    #[test]
    fn flushes_new_data_before_publishing_directory() {
        let mut fs = HermioneVolume::format(disk()).unwrap();
        fs.write_file("a", b"novo bloco").unwrap();
        let medium = fs.into_device();
        assert_eq!(medium.flush_after_write, vec![4, 6, 7, 8, 9]);
    }

    #[test]
    fn rejects_bad_names_missing_files_and_small_buffers() {
        let mut fs = HermioneVolume::format(disk()).unwrap();
        assert_eq!(fs.write_file("../secreto", b"x"), Err(FsError::InvalidName));
        assert_eq!(
            fs.read_file("vazio", &mut [0; 2]),
            Err(FsError::FileNotFound)
        );
        fs.write_file("a", b"abc").unwrap();
        assert_eq!(fs.read_file("a", &mut [0; 2]), Err(FsError::BufferTooSmall));
    }

    #[test]
    fn detects_data_corruption_and_frees_deleted_extent() {
        let mut fs = HermioneVolume::format(disk()).unwrap();
        fs.write_file("a", b"original").unwrap();
        assert_eq!(
            fs.scrub(),
            Ok(ScrubReport {
                files_verified: 1,
                copies_repaired: 0
            })
        );
        let mut disk = fs.into_device();
        disk.sectors[4][0] ^= 1;
        let mut fs = HermioneVolume::mount(disk).unwrap();
        let mut output = [0xff; 16];
        assert_eq!(fs.read_file("a", &mut output), Ok(8));
        assert_eq!(&output[..8], b"original");
        assert_eq!(
            fs.scrub(),
            Ok(ScrubReport {
                files_verified: 1,
                copies_repaired: 0
            })
        );
        let mut disk = fs.into_device();
        disk.sectors[4][0] ^= 1;
        disk.sectors[5][0] ^= 1;
        let mut fs = HermioneVolume::mount(disk).unwrap();
        assert_eq!(fs.scrub(), Err(FsError::ChecksumMismatch));
        fs.delete_file("a").unwrap();
        fs.write_file("b", b"replacement").unwrap();
        assert_eq!(fs.read_file("b", &mut [0; 16]), Ok(11));
    }

    #[test]
    fn scrub_repairs_a_bad_copy_only_from_the_verified_mirror() {
        let mut fs = HermioneVolume::format(disk()).unwrap();
        fs.write_file("a", b"redundant data").unwrap();
        let mut disk = fs.into_device();
        disk.sectors[4][0] ^= 0x80;
        let mut fs = HermioneVolume::mount(disk).unwrap();
        assert_eq!(
            fs.scrub(),
            Ok(ScrubReport {
                files_verified: 1,
                copies_repaired: 1
            })
        );
        let disk = fs.into_device();
        assert_eq!(disk.sectors[4], disk.sectors[5]);
    }

    #[test]
    fn mount_rejects_corrupt_superblock_and_directory() {
        {
            let fs = HermioneVolume::format(disk()).unwrap();
            let mut medium = fs.into_device();
            medium.sectors[0][8] ^= 1;
            assert_eq!(
                HermioneVolume::mount(medium).err(),
                Some(FsError::InvalidSuperblock)
            );
        }
        {
            let fs = HermioneVolume::format(disk()).unwrap();
            let mut medium = fs.into_device();
            medium.sectors[1][32] ^= 1;
            medium.sectors[2][32] ^= 1;
            assert_eq!(
                HermioneVolume::mount(medium).err(),
                Some(FsError::CorruptDirectory)
            );
        }
    }

    #[test]
    fn mount_rejects_invalid_duplicate_names_and_oversized_extents() {
        let fs = HermioneVolume::format(disk()).unwrap();
        let mut medium = fs.into_device();

        let mut entries = [Entry::EMPTY; ENTRY_COUNT];
        entries[0] = Entry {
            name: encode_name("a").unwrap(),
            start: DATA_START as u32,
            mirror_start: DATA_START as u32 + 1,
            sectors: 1,
            size: 1,
            checksum: 0,
        };
        entries[1] = Entry {
            name: encode_name("a").unwrap(),
            start: DATA_START as u32 + 1,
            mirror_start: DATA_START as u32 + 2,
            sectors: 1,
            size: 1,
            checksum: 0,
        };
        let duplicate = encode_directory(&entries, 1);
        medium.sectors[DIRECTORY_LBAS[0] as usize] = duplicate;
        medium.sectors[DIRECTORY_LBAS[1] as usize] = duplicate;
        assert_eq!(
            HermioneVolume::mount(medium).err(),
            Some(FsError::CorruptDirectory)
        );

        let fs = HermioneVolume::format(disk()).unwrap();
        let mut medium = fs.into_device();
        entries[1] = Entry::EMPTY;
        entries[0].sectors = 2;
        let oversized = encode_directory(&entries, 1);
        medium.sectors[DIRECTORY_LBAS[0] as usize] = oversized;
        medium.sectors[DIRECTORY_LBAS[1] as usize] = oversized;
        assert_eq!(
            HermioneVolume::mount(medium).err(),
            Some(FsError::CorruptDirectory)
        );

        let fs = HermioneVolume::format(disk()).unwrap();
        let mut medium = fs.into_device();
        entries[0].sectors = 1;
        entries[0].name[1] = b'/';
        let invalid_name = encode_directory(&entries, 1);
        medium.sectors[DIRECTORY_LBAS[0] as usize] = invalid_name;
        medium.sectors[DIRECTORY_LBAS[1] as usize] = invalid_name;
        assert_eq!(
            HermioneVolume::mount(medium).err(),
            Some(FsError::CorruptDirectory)
        );
    }

    #[test]
    fn failed_mirror_write_keeps_previous_directory_generation() {
        let mut fs = HermioneVolume::format(disk()).unwrap();
        let mut medium = fs.into_device();
        medium.tear_write_at = Some(medium.writes + 1); // segunda cópia de dados falha.
        fs = HermioneVolume::mount(medium).unwrap();
        assert_eq!(fs.write_file("novo", b"dados novos"), Err(FsError::Device));
        let medium = fs.into_device();
        let mut recovered = HermioneVolume::mount(medium).unwrap();
        let mut listed = [None; ENTRY_COUNT];
        assert_eq!(recovered.list(&mut listed), Ok(0));
        assert_eq!(
            recovered.read_file("novo", &mut [0; 16]),
            Err(FsError::FileNotFound)
        );
    }

    #[test]
    fn journal_redoes_commit_when_directory_write_is_torn() {
        let mut fs = HermioneVolume::format(disk()).unwrap();
        let mut medium = fs.into_device();
        medium.tear_write_at = Some(medium.writes + 3); // journal já foi flushado.
        fs = HermioneVolume::mount(medium).unwrap();
        assert_eq!(
            fs.write_file("novo", b"commit journal"),
            Err(FsError::Device)
        );
        let mut recovered = HermioneVolume::mount(fs.into_device()).unwrap();
        let mut output = [0; 32];
        let count = recovered.read_file("novo", &mut output).unwrap();
        assert_eq!(&output[..count], b"commit journal");
    }

    #[test]
    fn failed_replace_keeps_old_file_visible_after_mount() {
        let mut fs = HermioneVolume::format(disk()).unwrap();
        fs.write_file("cfg", b"versao antiga").unwrap();
        let mut medium = fs.into_device();
        medium.tear_write_at = Some(medium.writes + 1);
        let mut fs = HermioneVolume::mount(medium).unwrap();
        assert_eq!(fs.write_file("cfg", b"versao nova"), Err(FsError::Device));
        let medium = fs.into_device();
        let mut recovered = HermioneVolume::mount(medium).unwrap();
        let mut output = [0; 32];
        let count = recovered.read_file("cfg", &mut output).unwrap();
        assert_eq!(&output[..count], b"versao antiga");
    }

    #[test]
    fn interrupted_replace_at_each_write_and_flush_keeps_a_valid_generation() {
        let mut fs = HermioneVolume::format(disk()).unwrap();
        fs.write_file("cfg", b"versao antiga").unwrap();
        let baseline = fs.into_device();

        // Corte durante escrita de dados, durante commit do diretório e nos
        // dois flushes da transação. Remontar deve expor versão completa.
        for operation in 0..9 {
            let mut medium = baseline.clone();
            if operation < 5 {
                medium.tear_write_at = Some(medium.writes + operation);
            } else {
                medium.fail_flush_at = Some(medium.flushes + operation - 5);
            }
            let mut fs = HermioneVolume::mount(medium).unwrap();
            assert_eq!(fs.write_file("cfg", b"versao nova"), Err(FsError::Device));
            let mut medium = fs.into_device();
            medium.tear_write_at = None;
            medium.fail_flush_at = None;

            let mut recovered = HermioneVolume::mount(medium).unwrap();
            let mut output = [0; 32];
            let count = recovered.read_file("cfg", &mut output).unwrap();
            let contents = &output[..count];
            assert!(contents == b"versao antiga" || contents == b"versao nova");
            assert_eq!(
                recovered.scrub(),
                Ok(ScrubReport {
                    files_verified: 1,
                    copies_repaired: 0
                })
            );
        }
    }
}

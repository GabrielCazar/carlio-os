//! Volume Hermione experimental sobre dispositivos de setores de 512 bytes.
//!
//! Formato experimental sem cifra. Metadados usam duas cópias de diretório
//! versionadas; novos dados são escritos antes de publicar a cópia seguinte.
//! Ainda depende de gravação setorial confiável e não substitui um journal.

pub const SECTOR_SIZE: usize = 512;
const MAGIC: &[u8; 8] = b"HERMFS02";
const DIRECTORY_MAGIC: &[u8; 8] = b"HERM-DIR";
const VERSION: u32 = 2;
const DIRECTORY_LBAS: [u64; 2] = [1, 2];
const DIRECTORY_HEADER_SIZE: usize = 24;
const DATA_START: u64 = 3;
const ENTRY_SIZE: usize = 40;
const ENTRY_COUNT: usize = (SECTOR_SIZE - DIRECTORY_HEADER_SIZE) / ENTRY_SIZE;
const NAME_SIZE: usize = 24;

pub trait BlockDevice {
    fn sector_count(&self) -> u64;
    fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_SIZE]) -> Result<(), FsError>;
    fn write_sector(&mut self, lba: u64, data: &[u8; SECTOR_SIZE]) -> Result<(), FsError>;
    fn flush(&mut self) -> Result<(), FsError>;
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileInfo {
    pub name: [u8; NAME_SIZE],
    pub size: u32,
}

#[derive(Clone, Copy)]
struct Entry {
    name: [u8; NAME_SIZE],
    start: u32,
    sectors: u32,
    size: u32,
    checksum: u32,
}

impl Entry {
    const EMPTY: Self = Self {
        name: [0; NAME_SIZE],
        start: 0,
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
        if sectors <= DATA_START {
            return Err(FsError::TooSmall);
        }
        if sectors > u32::MAX as u64 {
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
            active_directory: DIRECTORY_LBAS[1],
            generation: 0,
        };
        let entries = [Entry::EMPTY; ENTRY_COUNT];
        let sector = encode_directory(&entries, 0);
        volume.device.write_sector(DIRECTORY_LBAS[0], &sector)?;
        volume.device.write_sector(DIRECTORY_LBAS[1], &sector)?;
        volume.device.flush()?;
        Ok(volume)
    }

    pub fn mount(mut device: D) -> Result<Self, FsError> {
        let device_sectors = device.sector_count();
        if device_sectors <= DATA_START {
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
            || stored_sectors > u32::MAX as u64
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
        let start = self
            .find_extent(&entries, sectors_needed.max(1) as u32)
            .ok_or(FsError::NoSpace)?;
        let mut checksum = !0u32;
        for n in 0..sectors_needed {
            let mut sector = [0; SECTOR_SIZE];
            let from = n * SECTOR_SIZE;
            let to = core::cmp::min(from + SECTOR_SIZE, data.len());
            sector[..to - from].copy_from_slice(&data[from..to]);
            checksum = crc32_update(checksum, &sector[..to - from]);
            self.device.write_sector(start as u64 + n as u64, &sector)?;
        }
        // Dados novos devem chegar à mídia antes da publicação do diretório.
        self.device.flush()?;
        entries[index] = Entry {
            name: encoded,
            start,
            sectors: sectors_needed.max(1) as u32,
            size: data.len() as u32,
            checksum: !checksum,
        };
        self.write_directory(&entries)
    }

    pub fn read_file(&mut self, name: &str, output: &mut [u8]) -> Result<usize, FsError> {
        let encoded = encode_name(name)?;
        let entries = self.read_directory()?;
        let entry = entries
            .iter()
            .find(|e| e.occupied() && e.name == encoded)
            .copied()
            .ok_or(FsError::FileNotFound)?;
        if output.len() < entry.size as usize {
            return Err(FsError::BufferTooSmall);
        }
        self.verify_entry(&entry)?;
        let mut remaining = entry.size as usize;
        let mut offset = 0;
        for n in 0..entry.sectors {
            let mut sector = [0; SECTOR_SIZE];
            if let Err(error) = self
                .device
                .read_sector(entry.start as u64 + n as u64, &mut sector)
            {
                output[..entry.size as usize].fill(0);
                return Err(error);
            }
            let used = core::cmp::min(remaining, SECTOR_SIZE);
            output[offset..offset + used].copy_from_slice(&sector[..used]);
            offset += used;
            remaining -= used;
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

    /// Verifica checksums de todos os arquivos sem expor dados ao chamador.
    pub fn scrub(&mut self) -> Result<usize, FsError> {
        let entries = self.read_directory()?;
        let mut verified = 0;
        for entry in entries.iter().filter(|entry| entry.occupied()) {
            self.verify_entry(entry)?;
            verified += 1;
        }
        Ok(verified)
    }

    fn verify_entry(&mut self, entry: &Entry) -> Result<(), FsError> {
        let mut remaining = entry.size as usize;
        let mut checksum = !0u32;
        for n in 0..entry.sectors {
            let mut sector = [0; SECTOR_SIZE];
            self.device
                .read_sector(entry.start as u64 + n as u64, &mut sector)?;
            let used = core::cmp::min(remaining, SECTOR_SIZE);
            checksum = crc32_update(checksum, &sector[..used]);
            remaining -= used;
        }
        if !checksum != entry.checksum {
            return Err(FsError::ChecksumMismatch);
        }
        Ok(())
    }

    fn find_extent(&self, entries: &[Entry; ENTRY_COUNT], count: u32) -> Option<u32> {
        let mut cursor = DATA_START;
        for _ in 0..=ENTRY_COUNT {
            let next = entries
                .iter()
                .enumerate()
                .filter(|(_, e)| e.occupied() && e.start as u64 >= cursor)
                .map(|(_, e)| e.start as u64)
                .min();
            if let Some(start) = next {
                if start.saturating_sub(cursor) >= count as u64 {
                    return Some(cursor as u32);
                }
                let extent_end = entries
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| e.occupied() && e.start as u64 == start)
                    .map(|(_, e)| e.start as u64 + e.sectors as u64)
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
        let mut candidates = [None, None];
        for (index, lba) in DIRECTORY_LBAS.iter().enumerate() {
            let mut sector = [0; SECTOR_SIZE];
            self.device.read_sector(*lba, &mut sector)?;
            candidates[index] = parse_directory(&sector, self.sectors);
        }
        let selected = match (candidates[0], candidates[1]) {
            (Some(a), Some(b)) if generation_is_newer(b.generation, a.generation) => {
                (DIRECTORY_LBAS[1], b)
            }
            (Some(a), _) => (DIRECTORY_LBAS[0], a),
            (None, Some(b)) => (DIRECTORY_LBAS[1], b),
            (None, None) => return Err(FsError::CorruptDirectory),
        };
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
        self.device.write_sector(target, &sector)?;
        self.device.flush()?;
        self.active_directory = target;
        self.generation = generation;
        Ok(())
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
        entry.sectors = get_u32(sector, p + 28);
        entry.size = get_u32(sector, p + 32);
        entry.checksum = get_u32(sector, p + 36);
        if entry.occupied() {
            let end = entry.start as u64 + entry.sectors as u64;
            if (entry.start as u64) < DATA_START
                || entry.sectors == 0
                || end > volume_sectors
                || entry.size as u64 > entry.sectors as u64 * SECTOR_SIZE as u64
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
            if (a.start as u64) < b.start as u64 + b.sectors as u64
                && (b.start as u64) < a.start as u64 + a.sectors as u64
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
        put_u32(&mut sector, p + 28, entry.sectors);
        put_u32(&mut sector, p + 32, entry.size);
        put_u32(&mut sector, p + 36, entry.checksum);
    }
    let checksum = crc32(&sector);
    put_u32(&mut sector, 12, checksum);
    sector
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
    struct MemDisk {
        sectors: Vec<[u8; SECTOR_SIZE]>,
        writes: usize,
        tear_write_at: Option<usize>,
        flush_after_write: Vec<usize>,
    }
    impl BlockDevice for MemDisk {
        fn sector_count(&self) -> u64 {
            self.sectors.len() as u64
        }
        fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_SIZE]) -> Result<(), FsError> {
            *out = *self.sectors.get(lba as usize).ok_or(FsError::Device)?;
            Ok(())
        }
        fn write_sector(&mut self, lba: u64, data: &[u8; SECTOR_SIZE]) -> Result<(), FsError> {
            let index = self.writes;
            self.writes += 1;
            let target = self.sectors.get_mut(lba as usize).ok_or(FsError::Device)?;
            if self.tear_write_at == Some(index) {
                target[..SECTOR_SIZE / 32].copy_from_slice(&data[..SECTOR_SIZE / 32]);
                return Err(FsError::Device);
            }
            *target = *data;
            Ok(())
        }
        fn flush(&mut self) -> Result<(), FsError> {
            self.flush_after_write.push(self.writes);
            Ok(())
        }
    }
    fn disk() -> MemDisk {
        MemDisk {
            sectors: vec![[0; SECTOR_SIZE]; 128],
            writes: 0,
            tear_write_at: None,
            flush_after_write: Vec::new(),
        }
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
        assert_eq!(medium.flush_after_write, vec![3, 4, 5]);
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
        assert_eq!(fs.scrub(), Ok(1));
        let mut disk = fs.into_device();
        disk.sectors[3][0] ^= 1;
        let mut fs = HermioneVolume::mount(disk).unwrap();
        let mut output = [0xff; 16];
        assert_eq!(
            fs.read_file("a", &mut output),
            Err(FsError::ChecksumMismatch)
        );
        assert_eq!(output, [0xff; 16]);
        assert_eq!(fs.scrub(), Err(FsError::ChecksumMismatch));
        fs.delete_file("a").unwrap();
        fs.write_file("b", b"replacement").unwrap();
        assert_eq!(fs.read_file("b", &mut [0; 16]), Ok(11));
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
    fn torn_directory_commit_falls_back_to_previous_generation() {
        let mut fs = HermioneVolume::format(disk()).unwrap();
        let mut medium = fs.into_device();
        medium.tear_write_at = Some(medium.writes + 1); // arquivo grava; diretório novo rasga.
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
}

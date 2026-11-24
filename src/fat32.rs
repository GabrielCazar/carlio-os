//! Leitor inicial FAT32 (somente leitura), sem heap e sobre setores de 512 B.
use crate::block::{BlockDevice, SECTOR_SIZE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fat32Error {
    Journal(crate::sector_journal::JournalError),
    WriteUnsupported,
    Device,
    InvalidBootSector,
    UnsupportedGeometry,
    InvalidCluster,
    CorruptChain,
    NotFound,
    BufferTooSmall,
    DirectoryFull,
    InvalidName,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fat32File {
    pub name: [u8; 11],
    pub size: u32,
    first_cluster: u32,
    directory_lba: u64,
    directory_offset: usize,
    pub attributes: u8,
}

pub struct Fat32Volume<D> {
    device: D,
    partition_start: u64,
    volume_sectors: u64,
    fat_start: u64,
    fat_sectors: u32,
    data_start: u64,
    cluster_count: u32,
    root_cluster: u32,
    sectors_per_cluster: u8,
}

const LFN_ENTRY_UNITS: usize = 13;
const LFN_MAX_ENTRIES: usize = 20;

struct LongName {
    units: [u16; LFN_ENTRY_UNITS * LFN_MAX_ENTRIES],
    count: usize,
    next: usize,
    checksum: u8,
}

impl LongName {
    fn new() -> Self {
        Self {
            units: [0; LFN_ENTRY_UNITS * LFN_MAX_ENTRIES],
            count: 0,
            next: 0,
            checksum: 0,
        }
    }

    fn push(&mut self, raw: &[u8; 32]) {
        let ordinal = usize::from(raw[0] & 0x1f);
        if raw[0] & 0xa0 != 0
            || !(1..=LFN_MAX_ENTRIES).contains(&ordinal)
            || raw[12] != 0
            || u16le(raw, 26) != 0
        {
            self.count = 0;
            return;
        }
        if raw[0] & 0x40 != 0 {
            self.count = ordinal;
            self.next = ordinal;
            self.checksum = raw[13];
        }
        if self.count == 0 || self.next != ordinal || self.checksum != raw[13] {
            self.count = 0;
            return;
        }
        let start = (ordinal - 1) * LFN_ENTRY_UNITS;
        for (index, offset) in [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30]
            .into_iter()
            .enumerate()
        {
            self.units[start + index] = u16le(raw, offset);
        }
        self.next -= 1;
    }

    fn for_alias(&self, alias: &[u8; 11]) -> Option<&[u16]> {
        let checksum = alias
            .iter()
            .fold(0u8, |sum, byte| sum.rotate_right(1).wrapping_add(*byte));
        if self.count == 0 || self.next != 0 || self.checksum != checksum {
            return None;
        }
        let units = &self.units[..self.count * LFN_ENTRY_UNITS];
        let length = units
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(units.len());
        if length <= (self.count - 1) * LFN_ENTRY_UNITS
            || length > 255
            || units[..length].contains(&0xffff)
            || units
                .get(length + 1..)
                .is_some_and(|padding| padding.iter().any(|unit| *unit != 0xffff))
        {
            return None;
        }
        Some(&units[..length])
    }
}

impl<D: BlockDevice> Fat32Volume<D> {
    /// Monta uma partição já identificada pelo chamador; não escreve no volume.
    pub fn mount(mut device: D, partition_start: u64) -> Result<Self, Fat32Error> {
        let mut boot = [0; SECTOR_SIZE];
        device
            .read_sector(partition_start, &mut boot)
            .map_err(|_| Fat32Error::Device)?;
        if boot[510] != 0x55 || boot[511] != 0xaa {
            return Err(Fat32Error::InvalidBootSector);
        }
        let bytes_per_sector = u16le(&boot, 11);
        let sectors_per_cluster = boot[13];
        let reserved = u16le(&boot, 14) as u32;
        let fats = boot[16];
        let total = if u16le(&boot, 19) != 0 {
            u16le(&boot, 19) as u32
        } else {
            u32le(&boot, 32)
        };
        let fat_sectors = u32le(&boot, 36);
        let flags = u16le(&boot, 40);
        let active_fat = if flags & 0x80 == 0 { 0 } else { flags & 0x0f };
        let root_cluster = u32le(&boot, 44) & 0x0fff_ffff;
        if bytes_per_sector != 512
            || !sectors_per_cluster.is_power_of_two()
            || sectors_per_cluster > 128
            || reserved == 0
            || !(1..=2).contains(&fats)
            || active_fat >= u16::from(fats)
            || fat_sectors == 0
            || total == 0
        {
            return Err(Fat32Error::UnsupportedGeometry);
        }
        let metadata_sectors = reserved as u64 + fats as u64 * fat_sectors as u64;
        if metadata_sectors >= total as u64
            || partition_start
                .checked_add(total as u64)
                .filter(|end| *end <= device.sector_count())
                .is_none()
        {
            return Err(Fat32Error::InvalidBootSector);
        }
        let data_sectors = total as u64 - metadata_sectors;
        let cluster_count = (data_sectors / sectors_per_cluster as u64) as u32;
        if !(65_525..=0x0fff_ffee).contains(&cluster_count)
            || root_cluster < 2
            || root_cluster >= cluster_count + 2
        {
            return Err(Fat32Error::UnsupportedGeometry);
        }
        let fat_capacity = fat_sectors as u64 * (SECTOR_SIZE as u64 / 4);
        if fat_capacity < cluster_count as u64 + 2 {
            return Err(Fat32Error::InvalidBootSector);
        }
        Ok(Self {
            device,
            partition_start,
            volume_sectors: total as u64,
            fat_start: partition_start
                + reserved as u64
                + u64::from(active_fat) * u64::from(fat_sectors),
            fat_sectors,
            data_start: partition_start + metadata_sectors,
            cluster_count,
            root_cluster,
            sectors_per_cluster,
        })
    }

    /// Overwrites one allocated sector of an existing file on a clean,
    /// exclusively owned volume. The caller reserves two sectors outside
    /// this volume for the redo record and must recover before other writes.
    pub fn overwrite_file_sector(
        &mut self,
        path: &str,
        journal: u64,
        data: &[u8; SECTOR_SIZE],
    ) -> Result<(), Fat32Error> {
        let target = self.writable_file_sector(path, journal)?;
        crate::sector_journal::overwrite(&mut self.device, journal, target, data)
            .map_err(Fat32Error::Journal)
    }

    pub fn recover_file_sector(&mut self, path: &str, journal: u64) -> Result<bool, Fat32Error> {
        let target = self.writable_file_sector(path, journal)?;
        crate::sector_journal::recover(&mut self.device, journal, target)
            .map_err(Fat32Error::Journal)
    }

    fn writable_file_sector(&mut self, path: &str, journal: u64) -> Result<u64, Fat32Error> {
        let file = self.resolve_file(path)?;
        if file.size < SECTOR_SIZE as u32
            || file.attributes & 0x09 != 0
            || journal == 0
            || journal
                .checked_add(1)
                .is_none_or(|last| last >= self.partition_start)
        {
            return Err(Fat32Error::WriteUnsupported);
        }
        self.next_cluster(file.first_cluster)?;
        self.cluster_sector(file.first_cluster)
    }

    /// Updates the on-disk read-only attribute of an existing file.
    /// The caller must exclusively own the volume and reserve the redo sectors.
    pub fn set_file_read_only(
        &mut self,
        path: &str,
        journal: u64,
        read_only: bool,
    ) -> Result<(), Fat32Error> {
        use crate::sector_journal::{overwrite_tagged, FAT32_ENTRY};

        self.check_metadata_journal(journal)?;
        let file = self.resolve_file(path)?;
        let mut sector = [0; SECTOR_SIZE];
        self.device
            .read_sector(file.directory_lba, &mut sector)
            .map_err(|_| Fat32Error::Device)?;
        sector[file.directory_offset + 11] = (file.attributes & !1) | u8::from(read_only);
        overwrite_tagged(
            &mut self.device,
            journal,
            file.directory_lba,
            &sector,
            FAT32_ENTRY,
        )
        .map_err(Fat32Error::Journal)
    }

    /// Replays a dedicated metadata journal before directory lookup. A torn
    /// directory sector can have lost the name needed to reach the record.
    pub fn recover_file_attributes(&mut self, journal: u64) -> Result<bool, Fat32Error> {
        use crate::sector_journal::{recover_tagged, FAT32_ENTRY};

        self.check_metadata_journal(journal)?;
        let data = self.data_start..self.partition_start + self.volume_sectors;
        recover_tagged(&mut self.device, journal, FAT32_ENTRY, |lba| {
            data.contains(&lba)
        })
        .map_err(Fat32Error::Journal)
    }

    fn check_metadata_journal(&mut self, journal: u64) -> Result<(), Fat32Error> {
        if journal == 0
            || journal
                .checked_add(1)
                .is_none_or(|last| last >= self.partition_start)
        {
            return Err(Fat32Error::WriteUnsupported);
        }
        let mut fat = [0; SECTOR_SIZE];
        self.device
            .read_sector(self.fat_start, &mut fat)
            .map_err(|_| Fat32Error::Device)?;
        if u32le(&fat, 4) & 0x0c00_0000 != 0x0c00_0000 {
            return Err(Fat32Error::WriteUnsupported);
        }
        Ok(())
    }

    pub fn into_device(self) -> D {
        self.device
    }

    pub fn list_root(&mut self, output: &mut [Option<Fat32File>]) -> Result<usize, Fat32Error> {
        let mut count = 0;
        self.walk_directory(self.root_cluster, |entry, _| {
            if entry.attributes & 0x08 != 0 || entry.attributes == 0x0f {
                return Ok(());
            }
            if count == output.len() {
                return Err(Fat32Error::DirectoryFull);
            }
            output[count] = Some(entry);
            count += 1;
            Ok(())
        })?;
        Ok(count)
    }

    pub fn read_root_file(&mut self, name: &str, output: &mut [u8]) -> Result<usize, Fat32Error> {
        self.read_file_in_directory(self.root_cluster, name, output)
    }

    /// Lê caminho relativo ao volume, incluindo diretórios 8.3.
    pub fn read_path(&mut self, path: &str, output: &mut [u8]) -> Result<usize, Fat32Error> {
        let file = self.resolve_file(path)?;
        if output.len() < file.size as usize {
            return Err(Fat32Error::BufferTooSmall);
        }
        self.read_file_entry_range(file, 0, output)
    }

    /// Reads a byte range by walking the FAT chain from the requested cluster.
    pub fn read_path_range(
        &mut self,
        path: &str,
        offset: u64,
        output: &mut [u8],
    ) -> Result<usize, Fat32Error> {
        let file = self.resolve_file(path)?;
        self.read_file_entry_range(file, offset, output)
    }

    fn resolve_file(&mut self, path: &str) -> Result<Fat32File, Fat32Error> {
        let entry = self.resolve_entry(path)?;
        if entry.attributes & 0x10 != 0 {
            return Err(Fat32Error::NotFound);
        }
        Ok(entry)
    }

    fn resolve_entry(&mut self, path: &str) -> Result<Fat32File, Fat32Error> {
        let mut entry = Fat32File {
            first_cluster: self.root_cluster,
            attributes: 0x10,
            size: 0,
            name: [b' '; 11],
            directory_lba: 0,
            directory_offset: 0,
        };
        for component in path.split('/').filter(|part| !part.is_empty()) {
            if entry.attributes & 0x10 == 0 {
                return Err(Fat32Error::NotFound);
            }
            entry = self.find_child(entry.first_cluster, component)?;
        }
        Ok(entry)
    }

    fn read_file_in_directory(
        &mut self,
        directory: u32,
        name: &str,
        output: &mut [u8],
    ) -> Result<usize, Fat32Error> {
        let file = self.find_child(directory, name)?;
        if file.attributes & 0x10 != 0 {
            return Err(Fat32Error::NotFound);
        }
        self.read_file_entry(file, output)
    }

    fn read_file_entry(&mut self, file: Fat32File, output: &mut [u8]) -> Result<usize, Fat32Error> {
        if output.len() < file.size as usize {
            return Err(Fat32Error::BufferTooSmall);
        }
        self.read_file_entry_range(file, 0, output)
    }

    fn read_file_entry_range(
        &mut self,
        file: Fat32File,
        offset: u64,
        output: &mut [u8],
    ) -> Result<usize, Fat32Error> {
        if file.size == 0 {
            return Ok(0);
        }
        if offset >= u64::from(file.size) || output.is_empty() {
            return Ok(0);
        }
        let mut cluster = file.first_cluster;
        let cluster_bytes = u64::from(self.sectors_per_cluster) * SECTOR_SIZE as u64;
        let mut skip_clusters = offset / cluster_bytes;
        let mut cluster_offset = (offset % cluster_bytes) as usize;
        let mut visited = 0u32;
        while skip_clusters > 0 {
            if visited >= self.cluster_count {
                return Err(Fat32Error::CorruptChain);
            }
            cluster = self
                .next_cluster(cluster)?
                .ok_or(Fat32Error::CorruptChain)?;
            visited += 1;
            skip_clusters -= 1;
        }
        let file_remaining = usize::try_from(u64::from(file.size) - offset)
            .map_err(|_| Fat32Error::BufferTooSmall)?;
        let mut remaining = output.len().min(file_remaining);
        let mut written = 0;
        while remaining > 0 {
            self.validate_cluster(cluster)?;
            if visited >= self.cluster_count {
                return Err(Fat32Error::CorruptChain);
            }
            visited += 1;
            let first_sector = cluster_offset / SECTOR_SIZE;
            for sector_index in first_sector..self.sectors_per_cluster as usize {
                if remaining == 0 {
                    break;
                }
                let mut sector = [0; SECTOR_SIZE];
                self.device
                    .read_sector(
                        self.cluster_sector(cluster)? + sector_index as u64,
                        &mut sector,
                    )
                    .map_err(|_| Fat32Error::Device)?;
                let sector_offset = if sector_index == first_sector {
                    cluster_offset % SECTOR_SIZE
                } else {
                    0
                };
                let used = remaining.min(SECTOR_SIZE - sector_offset);
                output[written..written + used]
                    .copy_from_slice(&sector[sector_offset..sector_offset + used]);
                written += used;
                remaining -= used;
            }
            cluster_offset = 0;
            if remaining > 0 {
                cluster = self
                    .next_cluster(cluster)?
                    .ok_or(Fat32Error::CorruptChain)?;
            }
        }
        Ok(written)
    }

    fn find_child(&mut self, directory: u32, name: &str) -> Result<Fat32File, Fat32Error> {
        if name.encode_utf16().count() > 255 || name.contains('\0') {
            return Err(Fat32Error::InvalidName);
        }
        let encoded = encode_short_name(name).ok();
        let mut found = None;
        self.walk_directory(directory, |entry, long_name| {
            if found.is_none()
                && entry.attributes & 0x08 == 0
                && (encoded == Some(entry.name)
                    || long_name.is_some_and(|units| {
                        char::decode_utf16(units.iter().copied())
                            .map(|ch| ch.map(|ch| ch.to_ascii_lowercase()))
                            .eq(name.chars().map(|ch| Ok(ch.to_ascii_lowercase())))
                    }))
            {
                found = Some(entry);
            }
            Ok(())
        })?;
        found.ok_or(Fat32Error::NotFound)
    }

    fn walk_directory(
        &mut self,
        first: u32,
        mut visit: impl FnMut(Fat32File, Option<&[u16]>) -> Result<(), Fat32Error>,
    ) -> Result<(), Fat32Error> {
        let mut cluster = first;
        let mut long_name = LongName::new();
        for _ in 0..self.cluster_count {
            self.validate_cluster(cluster)?;
            for sector_index in 0..self.sectors_per_cluster {
                let mut sector = [0; SECTOR_SIZE];
                let directory_lba = self.cluster_sector(cluster)? + u64::from(sector_index);
                self.device
                    .read_sector(directory_lba, &mut sector)
                    .map_err(|_| Fat32Error::Device)?;
                for (index, raw) in sector.as_chunks::<32>().0.iter().enumerate() {
                    if raw[0] == 0 {
                        return Ok(());
                    }
                    if raw[0] == 0xe5 {
                        long_name.count = 0;
                        continue;
                    }
                    if raw[11] == 0x0f {
                        long_name.push(raw);
                        continue;
                    }
                    let mut name = [0; 11];
                    name.copy_from_slice(&raw[..11]);
                    let first_cluster =
                        (((u16le(raw, 20) as u32) << 16) | u16le(raw, 26) as u32) & 0x0fff_ffff;
                    visit(
                        Fat32File {
                            name,
                            size: u32le(raw, 28),
                            first_cluster,
                            directory_lba,
                            directory_offset: index * 32,
                            attributes: raw[11],
                        },
                        long_name.for_alias(&name),
                    )?;
                    long_name.count = 0;
                }
            }
            match self.next_cluster(cluster)? {
                Some(next) => cluster = next,
                None => return Ok(()),
            }
        }
        Err(Fat32Error::CorruptChain)
    }

    fn validate_cluster(&self, cluster: u32) -> Result<(), Fat32Error> {
        if cluster < 2 || cluster >= self.cluster_count + 2 {
            Err(Fat32Error::InvalidCluster)
        } else {
            Ok(())
        }
    }

    fn cluster_sector(&self, cluster: u32) -> Result<u64, Fat32Error> {
        self.validate_cluster(cluster)?;
        let relative = self
            .data_start
            .checked_add((cluster as u64 - 2) * self.sectors_per_cluster as u64)
            .ok_or(Fat32Error::InvalidCluster)?;
        if relative >= self.partition_start + self.volume_sectors {
            return Err(Fat32Error::InvalidCluster);
        }
        Ok(relative)
    }

    fn next_cluster(&mut self, cluster: u32) -> Result<Option<u32>, Fat32Error> {
        self.validate_cluster(cluster)?;
        let offset = cluster as u64 * 4;
        let sector_index = offset / SECTOR_SIZE as u64;
        if sector_index >= self.fat_sectors as u64 {
            return Err(Fat32Error::InvalidCluster);
        }
        let mut sector = [0; SECTOR_SIZE];
        self.device
            .read_sector(self.fat_start + sector_index, &mut sector)
            .map_err(|_| Fat32Error::Device)?;
        let value = u32::from_le_bytes(
            sector[(offset as usize % SECTOR_SIZE)..][..4]
                .try_into()
                .unwrap(),
        ) & 0x0fff_ffff;
        if value >= 0x0fff_fff8 {
            return Ok(None);
        }
        self.validate_cluster(value)
            .map_err(|_| Fat32Error::CorruptChain)?;
        Ok(Some(value))
    }
}

impl<D: BlockDevice> crate::vfs::ReadOnlyMountBackend for Fat32Volume<D> {
    fn filesystem_type(&self) -> crate::vfs::FilesystemType {
        crate::vfs::FilesystemType::Fat32
    }

    fn read_file(&mut self, path: &str, output: &mut [u8]) -> Result<usize, crate::vfs::VfsError> {
        self.read_path(path, output).map_err(|error| match error {
            Fat32Error::NotFound => crate::vfs::VfsError::NotFound,
            Fat32Error::BufferTooSmall => crate::vfs::VfsError::BufferTooSmall,
            Fat32Error::InvalidName => crate::vfs::VfsError::InvalidPath,
            _ => crate::vfs::VfsError::InvalidPath,
        })
    }

    fn metadata(&mut self, path: &str) -> Result<crate::vfs::MountMetadata, crate::vfs::VfsError> {
        let file = self.resolve_entry(path).map_err(|error| match error {
            Fat32Error::NotFound => crate::vfs::VfsError::NotFound,
            Fat32Error::InvalidName => crate::vfs::VfsError::InvalidPath,
            _ => crate::vfs::VfsError::InvalidPath,
        })?;
        Ok(crate::vfs::MountMetadata {
            inode: u64::from(file.first_cluster),
            kind: if file.attributes & 0x10 != 0 {
                crate::vfs::NodeKind::Directory
            } else {
                crate::vfs::NodeKind::File
            },
            uid: 0,
            gid: 0,
            mode: if file.attributes & 0x10 != 0 {
                0o755
            } else if file.attributes & 0x01 != 0 {
                0o444
            } else {
                0o666
            },
            size: u64::from(file.size),
        })
    }

    fn read_file_range(
        &mut self,
        path: &str,
        offset: u64,
        output: &mut [u8],
    ) -> Result<usize, crate::vfs::VfsError> {
        self.read_path_range(path, offset, output)
            .map_err(|error| match error {
                Fat32Error::NotFound => crate::vfs::VfsError::NotFound,
                Fat32Error::BufferTooSmall => crate::vfs::VfsError::BufferTooSmall,
                Fat32Error::InvalidName => crate::vfs::VfsError::InvalidPath,
                _ => crate::vfs::VfsError::InvalidPath,
            })
    }
}

fn encode_short_name(name: &str) -> Result<[u8; 11], Fat32Error> {
    let mut parts = name.split('.');
    let base = parts.next().unwrap_or("");
    let ext = parts.next().unwrap_or("");
    if base.is_empty()
        || base.len() > 8
        || ext.len() > 3
        || parts.next().is_some()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".$%'-_@~`!(){}^#&".contains(&b))
    {
        return Err(Fat32Error::InvalidName);
    }
    let mut encoded = [b' '; 11];
    for (dst, src) in encoded[..8].iter_mut().zip(base.bytes()) {
        *dst = src.to_ascii_uppercase();
    }
    for (dst, src) in encoded[8..].iter_mut().zip(ext.bytes()) {
        *dst = src.to_ascii_uppercase();
    }
    Ok(encoded)
}

fn u16le(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}
fn u32le(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct SparseDisk {
        sectors: BTreeMap<u64, [u8; SECTOR_SIZE]>,
        count: u64,
    }

    impl BlockDevice for SparseDisk {
        fn sector_count(&self) -> u64 {
            self.count
        }
        fn read_sector(
            &mut self,
            lba: u64,
            out: &mut [u8; SECTOR_SIZE],
        ) -> Result<(), crate::block::BlockError> {
            if lba >= self.count {
                return Err(crate::block::BlockError::Device);
            }
            *out = self.sectors.get(&lba).copied().unwrap_or([0; SECTOR_SIZE]);
            Ok(())
        }
        fn write_sector(
            &mut self,
            lba: u64,
            data: &[u8; SECTOR_SIZE],
        ) -> Result<(), crate::block::BlockError> {
            if lba >= self.count {
                return Err(crate::block::BlockError::Device);
            }
            self.sectors.insert(lba, *data);
            Ok(())
        }
        fn flush(&mut self) -> Result<(), crate::block::BlockError> {
            Ok(())
        }
    }

    fn sample_volume() -> SparseDisk {
        let mut disk = SparseDisk {
            sectors: BTreeMap::new(),
            count: 70_010,
        };
        let start = 10;
        let mut boot = [0; SECTOR_SIZE];
        boot[11..13].copy_from_slice(&512u16.to_le_bytes());
        boot[13] = 1;
        boot[14..16].copy_from_slice(&1u16.to_le_bytes());
        boot[16] = 1;
        boot[32..36].copy_from_slice(&70_000u32.to_le_bytes());
        boot[36..40].copy_from_slice(&600u32.to_le_bytes());
        boot[44..48].copy_from_slice(&2u32.to_le_bytes());
        boot[510] = 0x55;
        boot[511] = 0xaa;
        disk.sectors.insert(start, boot);

        let mut fat = [0; SECTOR_SIZE];
        fat[8..12].copy_from_slice(&0x0fff_fff8u32.to_le_bytes());
        fat[12..16].copy_from_slice(&0x0fff_fff8u32.to_le_bytes());
        fat[16..20].copy_from_slice(&0x0fff_fff8u32.to_le_bytes());
        fat[20..24].copy_from_slice(&0x0fff_fff8u32.to_le_bytes());
        disk.sectors.insert(start + 1, fat);

        let mut root = [0; SECTOR_SIZE];
        root[..11].copy_from_slice(b"README  TXT");
        root[11] = 0x20;
        root[26..28].copy_from_slice(&3u16.to_le_bytes());
        root[28..32].copy_from_slice(&5u32.to_le_bytes());
        root[32..43].copy_from_slice(b"DOCS       ");
        root[43] = 0x10;
        root[58..60].copy_from_slice(&4u16.to_le_bytes());
        disk.sectors.insert(start + 601, root);

        let mut directory = [0; SECTOR_SIZE];
        directory[..11].copy_from_slice(b"NOTE    TXT");
        directory[11] = 0x20;
        directory[26..28].copy_from_slice(&5u16.to_le_bytes());
        directory[28..32].copy_from_slice(&6u32.to_le_bytes());
        disk.sectors.insert(start + 603, directory);

        let mut content = [0; SECTOR_SIZE];
        content[..5].copy_from_slice(b"beta!");
        disk.sectors.insert(start + 602, content);
        let mut nested_content = [0; SECTOR_SIZE];
        nested_content[..6].copy_from_slice(b"nested");
        disk.sectors.insert(start + 604, nested_content);
        disk
    }

    #[test]
    fn mounts_partition_and_reads_root_83_file_without_writes() {
        let disk = sample_volume();
        let original = disk.sectors.clone();
        let mut volume = Fat32Volume::mount(disk, 10).unwrap();
        let mut entries = [None; 4];
        assert_eq!(volume.list_root(&mut entries), Ok(2));
        assert_eq!(entries[0].unwrap().name, *b"README  TXT");
        let mut output = [0; 8];
        assert_eq!(volume.read_root_file("readme.txt", &mut output), Ok(5));
        assert_eq!(&output[..5], b"beta!");
        assert_eq!(volume.read_path("/readme.txt", &mut output), Ok(5));
        assert_eq!(volume.into_device().sectors, original);
    }

    #[test]
    fn reads_files_in_subdirectories_by_short_name() {
        let mut volume = Fat32Volume::mount(sample_volume(), 10).unwrap();
        let mut output = [0; 8];
        assert_eq!(volume.read_path("/docs/note.txt", &mut output), Ok(6));
        assert_eq!(&output[..6], b"nested");
    }

    #[test]
    fn reads_long_name_and_retains_alias_when_checksum_is_invalid() {
        let mut disk = sample_volume();
        let alias = *b"LONGNA~1TXT";
        let name = "Long name 🚀.txt";
        let mut units = [0xffffu16; 26];
        let length = name.encode_utf16().count();
        for (target, unit) in units.iter_mut().zip(name.encode_utf16()) {
            *target = unit;
        }
        units[length] = 0;
        let root = disk.sectors.get_mut(&611).unwrap();
        root.copy_within(..32, 64);
        root[64..75].copy_from_slice(&alias);
        root[96..].fill(0);
        let checksum = alias
            .iter()
            .fold(0u8, |sum, byte| sum.rotate_right(1).wrapping_add(*byte));
        for (slot, ordinal) in [2usize, 1].into_iter().enumerate() {
            let raw = &mut root[slot * 32..(slot + 1) * 32];
            raw.fill(0);
            raw[0] = ordinal as u8 | if slot == 0 { 0x40 } else { 0 };
            raw[11] = 0x0f;
            raw[13] = checksum;
            for (index, offset) in [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30]
                .into_iter()
                .enumerate()
            {
                raw[offset..offset + 2]
                    .copy_from_slice(&units[(ordinal - 1) * 13 + index].to_le_bytes());
            }
        }
        let mut volume = Fat32Volume::mount(disk, 10).unwrap();
        let mut output = [0; 5];
        assert_eq!(
            volume.read_root_file("long NAME 🚀.TXT", &mut output),
            Ok(5)
        );
        assert_eq!(&output, b"beta!");
        let mut disk = volume.into_device();
        disk.sectors.get_mut(&611).unwrap()[13] ^= 1;
        let mut volume = Fat32Volume::mount(disk, 10).unwrap();
        assert_eq!(
            volume.read_root_file(name, &mut output),
            Err(Fat32Error::NotFound)
        );
        assert_eq!(volume.read_root_file("LONGNA~1.TXT", &mut output), Ok(5));
    }

    #[test]
    fn uses_active_fat_and_distinguishes_bad_cluster_from_end_of_chain() {
        let mut disk = sample_volume();
        let boot = disk.sectors.get_mut(&10).unwrap();
        boot[16] = 2;
        boot[40..42].copy_from_slice(&0x81u16.to_le_bytes());
        let fat = disk.sectors.remove(&11).unwrap();
        for sector in (611..=614).rev() {
            let data = disk.sectors.remove(&sector).unwrap();
            disk.sectors.insert(sector + 600, data);
        }
        let root = disk.sectors.get_mut(&1211).unwrap();
        for entry in root[64..].as_chunks_mut::<32>().0 {
            entry[0] = 0xe5;
        }
        disk.sectors.insert(611, fat);
        let mut volume = Fat32Volume::mount(disk, 10).unwrap();
        assert_eq!(volume.list_root(&mut [None; 2]), Ok(2));
        let mut disk = volume.into_device();
        disk.sectors.get_mut(&611).unwrap()[8..12].copy_from_slice(&0x0fff_fff7u32.to_le_bytes());
        let mut volume = Fat32Volume::mount(disk, 10).unwrap();
        assert_eq!(
            volume.list_root(&mut [None; 2]),
            Err(Fat32Error::CorruptChain)
        );
    }

    #[test]
    fn rejects_bad_geometry_and_out_of_partition_reads() {
        let mut disk = sample_volume();
        disk.sectors.get_mut(&10).unwrap()[11..13].copy_from_slice(&1024u16.to_le_bytes());
        assert_eq!(
            Fat32Volume::mount(disk, 10).err(),
            Some(Fat32Error::UnsupportedGeometry)
        );
        assert_eq!(
            Fat32Volume::mount(sample_volume(), 69_999).err(),
            Some(Fat32Error::InvalidBootSector)
        );
    }
}

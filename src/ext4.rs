//! Leitor ext4 somente leitura para volumes simples com blocos de 1–4 KiB.
//!
//! Suporta lookup linear de diretórios e extents armazenados no inode. Não
//! recupera journal nem interpreta recursos avançados; volumes com recursos
//! incompatíveis devem continuar sendo montados somente em outro sistema.
use crate::block::{BlockDevice, SECTOR_SIZE};

const MAX_BLOCK_SIZE: usize = 4096;
const EXT4_MAGIC: u16 = 0xef53;
const EXT4_EXTENTS_FL: u32 = 0x0008_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ext4Error {
    Device,
    InvalidSuperblock,
    UnsupportedFeature,
    CorruptMetadata,
    NotFound,
    IsDirectory,
    BufferTooSmall,
    InvalidPath,
}

pub struct Ext4Volume<D> {
    device: D,
    partition_start: u64,
    block_size: usize,
    blocks_count: u64,
    inodes_count: u32,
    inodes_per_group: u32,
    inode_size: usize,
    descriptor_size: usize,
}

#[derive(Clone, Copy)]
struct Inode {
    number: u32,
    mode: u16,
    flags: u32,
    uid: u32,
    gid: u32,
    size: u64,
    block: [u8; 60],
}

impl<D: BlockDevice> Ext4Volume<D> {
    /// Monta uma partição ext4 já identificada pelo chamador sem escrita.
    pub fn mount(mut device: D, partition_start: u64) -> Result<Self, Ext4Error> {
        let mut sb = [0u8; 1024];
        for sector in 0..2 {
            let mut data = [0u8; SECTOR_SIZE];
            device
                .read_sector(partition_start + 2 + sector, &mut data)
                .map_err(|_| Ext4Error::Device)?;
            sb[sector as usize * SECTOR_SIZE..(sector as usize + 1) * SECTOR_SIZE]
                .copy_from_slice(&data);
        }
        if u16le(&sb, 56) != EXT4_MAGIC {
            return Err(Ext4Error::InvalidSuperblock);
        }
        let log_block_size = u32le(&sb, 24);
        if log_block_size > 2 {
            return Err(Ext4Error::UnsupportedFeature);
        }
        let block_size = 1024usize << log_block_size;
        let incompat = u32le(&sb, 96);
        let supported_incompat = 0x0002 | 0x0040 | 0x0080;
        if incompat & !supported_incompat != 0 {
            return Err(Ext4Error::UnsupportedFeature);
        }
        let blocks_count = u32le(&sb, 4) as u64 | ((u32le(&sb, 336) as u64) << 32);
        let inodes_count = u32le(&sb, 0);
        let blocks_per_group = u32le(&sb, 32);
        let inodes_per_group = u32le(&sb, 40);
        let inode_size = u16le(&sb, 88) as usize;
        let descriptor_size = if incompat & 0x0080 != 0 {
            let size = u16le(&sb, 254) as usize;
            if size < 32 || size > block_size {
                return Err(Ext4Error::UnsupportedFeature);
            }
            size
        } else {
            32
        };
        let volume_sectors = blocks_count
            .checked_mul((block_size / SECTOR_SIZE) as u64)
            .ok_or(Ext4Error::InvalidSuperblock)?;
        if !(1024..=MAX_BLOCK_SIZE).contains(&block_size)
            || blocks_count == 0
            || inodes_count == 0
            || blocks_per_group == 0
            || inodes_per_group == 0
            || inode_size < 128
            || inode_size > block_size
            || partition_start
                .checked_add(volume_sectors)
                .filter(|end| *end <= device.sector_count())
                .is_none()
        {
            return Err(Ext4Error::InvalidSuperblock);
        }
        Ok(Self {
            device,
            partition_start,
            block_size,
            blocks_count,
            inodes_count,
            inodes_per_group,
            inode_size,
            descriptor_size,
        })
    }

    pub fn into_device(self) -> D {
        self.device
    }

    pub fn read_path(&mut self, path: &str, output: &mut [u8]) -> Result<usize, Ext4Error> {
        let inode = self.resolve_file(path)?;
        if inode.size > output.len() as u64 {
            return Err(Ext4Error::BufferTooSmall);
        }
        self.read_inode_range(&inode, 0, output)
    }

    /// Reads a file range directly from extents, without allocating a buffer
    /// proportional to the file's total size.
    pub fn read_path_range(
        &mut self,
        path: &str,
        offset: u64,
        output: &mut [u8],
    ) -> Result<usize, Ext4Error> {
        let inode = self.resolve_file(path)?;
        self.read_inode_range(&inode, offset, output)
    }

    fn resolve_inode(&mut self, path: &str) -> Result<Inode, Ext4Error> {
        let mut inode_number = 2u32;
        for component in path.split('/').filter(|part| !part.is_empty()) {
            let directory = self.read_inode(inode_number)?;
            if directory.mode & 0xf000 != 0x4000 {
                return Err(Ext4Error::NotFound);
            }
            inode_number = self.find_directory_entry(directory, component)?;
        }
        self.read_inode(inode_number)
    }

    fn resolve_file(&mut self, path: &str) -> Result<Inode, Ext4Error> {
        let inode = self.resolve_inode(path)?;
        if inode.mode & 0xf000 == 0x4000 {
            return Err(Ext4Error::IsDirectory);
        }
        if inode.mode & 0xf000 != 0x8000 {
            return Err(Ext4Error::NotFound);
        }
        Ok(inode)
    }

    fn read_inode_range(
        &mut self,
        inode: &Inode,
        offset: u64,
        output: &mut [u8],
    ) -> Result<usize, Ext4Error> {
        if offset >= inode.size || output.is_empty() {
            return Ok(0);
        }
        let available =
            usize::try_from(inode.size - offset).map_err(|_| Ext4Error::BufferTooSmall)?;
        let count = output.len().min(available);
        let block_size = self.block_size as u64;
        let mut logical =
            u32::try_from(offset / block_size).map_err(|_| Ext4Error::CorruptMetadata)?;
        let mut block_offset = (offset % block_size) as usize;
        let mut written = 0usize;
        while written < count {
            let used = (count - written).min(self.block_size - block_offset);
            let destination = &mut output[written..written + used];
            if let Some(physical) = self.extent_block(inode, logical)? {
                let mut block = [0u8; MAX_BLOCK_SIZE];
                self.read_block(physical, &mut block[..self.block_size])?;
                destination.copy_from_slice(&block[block_offset..block_offset + used]);
            } else {
                destination.fill(0);
            }
            written += used;
            block_offset = 0;
            logical = logical.checked_add(1).ok_or(Ext4Error::CorruptMetadata)?;
        }
        Ok(written)
    }

    fn read_inode(&mut self, number: u32) -> Result<Inode, Ext4Error> {
        if number == 0 || number > self.inodes_count {
            return Err(Ext4Error::CorruptMetadata);
        }
        let index = number - 1;
        let group = index / self.inodes_per_group;
        let local = index % self.inodes_per_group;
        let descriptor_size = self.block_size;
        let group_table_block = if self.block_size == 1024 { 2 } else { 1 };
        let descriptor_offset = group as usize * self.descriptor_size;
        let descriptor_block =
            group_table_block as u64 + (descriptor_offset / descriptor_size) as u64;
        let in_block = descriptor_offset % descriptor_size;
        let mut descriptor = [0u8; MAX_BLOCK_SIZE];
        self.read_block(descriptor_block, &mut descriptor[..self.block_size])?;
        let inode_table = u32le(&descriptor, in_block + 8) as u64
            | if self.descriptor_size >= 64 {
                (u32le(&descriptor, in_block + 40) as u64) << 32
            } else {
                0
            };
        let byte_offset = local as u64 * self.inode_size as u64;
        let inode_block = inode_table + byte_offset / self.block_size as u64;
        let inode_offset = (byte_offset % self.block_size as u64) as usize;
        let mut data = [0u8; MAX_BLOCK_SIZE];
        self.read_block(inode_block, &mut data[..self.block_size])?;
        if inode_offset + self.inode_size > self.block_size {
            return Err(Ext4Error::CorruptMetadata);
        }
        let mut block = [0u8; 60];
        block.copy_from_slice(&data[inode_offset + 40..inode_offset + 100]);
        let size_lo = u32le(&data, inode_offset + 4) as u64;
        let size_hi = if u16le(&data, inode_offset) & 0xf000 == 0x8000 {
            u32le(&data, inode_offset + 108) as u64
        } else {
            0
        };
        Ok(Inode {
            number,
            mode: u16le(&data, inode_offset),
            flags: u32le(&data, inode_offset + 32),
            uid: u16le(&data, inode_offset + 2) as u32
                | ((u16le(&data, inode_offset + 120) as u32) << 16),
            gid: u16le(&data, inode_offset + 24) as u32
                | ((u16le(&data, inode_offset + 122) as u32) << 16),
            size: size_lo | (size_hi << 32),
            block,
        })
    }

    fn find_directory_entry(&mut self, directory: Inode, name: &str) -> Result<u32, Ext4Error> {
        if !name.is_ascii() || name.len() > 255 {
            return Err(Ext4Error::InvalidPath);
        }
        let blocks = directory.size.div_ceil(self.block_size as u64);
        for logical in 0..blocks {
            let physical = self
                .extent_block(&directory, logical as u32)?
                .ok_or(Ext4Error::CorruptMetadata)?;
            let mut data = [0u8; MAX_BLOCK_SIZE];
            self.read_block(physical, &mut data[..self.block_size])?;
            let mut offset = 0usize;
            while offset + 8 <= self.block_size {
                let inode = u32le(&data, offset);
                let record_len = u16le(&data, offset + 4) as usize;
                let name_len = data[offset + 6] as usize;
                if record_len < 8
                    || record_len & 3 != 0
                    || offset + record_len > self.block_size
                    || name_len > record_len - 8
                {
                    return Err(Ext4Error::CorruptMetadata);
                }
                if inode != 0
                    && name_len == name.len()
                    && &data[offset + 8..offset + 8 + name_len] == name.as_bytes()
                {
                    return Ok(inode);
                }
                offset += record_len;
            }
        }
        Err(Ext4Error::NotFound)
    }

    fn extent_block(&mut self, inode: &Inode, logical: u32) -> Result<Option<u64>, Ext4Error> {
        if inode.flags & EXT4_EXTENTS_FL == 0 {
            return Err(Ext4Error::UnsupportedFeature);
        }
        let mut node = inode.block;
        for _ in 0..6 {
            let entries = u16le(&node, 2) as usize;
            let maximum = u16le(&node, 4) as usize;
            let depth = u16le(&node, 6);
            if u16le(&node, 0) != 0xf30a || entries > maximum || maximum > (node.len() - 12) / 12 {
                return Err(Ext4Error::CorruptMetadata);
            }
            if depth == 0 {
                for index in 0..entries {
                    let at = 12 + index * 12;
                    let first = u32le(&node, at);
                    let encoded_length = u16le(&node, at + 4);
                    let unwritten = encoded_length > 32_768;
                    let length = if unwritten {
                        encoded_length - 32_768
                    } else {
                        encoded_length
                    };
                    if length == 0 {
                        return Err(Ext4Error::CorruptMetadata);
                    }
                    let physical =
                        u32le(&node, at + 8) as u64 | ((u16le(&node, at + 6) as u64) << 32);
                    if logical >= first && (logical as u64) < first as u64 + length as u64 {
                        let block = physical + (logical - first) as u64;
                        if block >= self.blocks_count {
                            return Err(Ext4Error::CorruptMetadata);
                        }
                        return Ok((!unwritten).then_some(block));
                    }
                }
                return Ok(None);
            }
            let mut selected = None;
            for index in 0..entries {
                let at = 12 + index * 12;
                if u32le(&node, at) <= logical {
                    selected =
                        Some(u32le(&node, at + 4) as u64 | ((u16le(&node, at + 8) as u64) << 32));
                } else {
                    break;
                }
            }
            let Some(block) = selected else {
                return Ok(None);
            };
            if block >= self.blocks_count {
                return Err(Ext4Error::CorruptMetadata);
            }
            self.read_block(block, &mut node[..self.block_size])?;
        }
        Err(Ext4Error::CorruptMetadata)
    }

    fn read_block(&mut self, block: u64, output: &mut [u8]) -> Result<(), Ext4Error> {
        if block >= self.blocks_count || output.len() != self.block_size {
            return Err(Ext4Error::CorruptMetadata);
        }
        let first = self
            .partition_start
            .checked_add(block * (self.block_size / SECTOR_SIZE) as u64)
            .ok_or(Ext4Error::CorruptMetadata)?;
        for index in 0..self.block_size / SECTOR_SIZE {
            let mut sector = [0u8; SECTOR_SIZE];
            self.device
                .read_sector(first + index as u64, &mut sector)
                .map_err(|_| Ext4Error::Device)?;
            output[index * SECTOR_SIZE..(index + 1) * SECTOR_SIZE].copy_from_slice(&sector);
        }
        Ok(())
    }
}

impl<D: BlockDevice> crate::vfs::ReadOnlyMountBackend for Ext4Volume<D> {
    fn filesystem_type(&self) -> crate::vfs::FilesystemType {
        crate::vfs::FilesystemType::Ext4
    }
    fn read_file(&mut self, path: &str, output: &mut [u8]) -> Result<usize, crate::vfs::VfsError> {
        self.read_path(path, output).map_err(|error| match error {
            Ext4Error::NotFound => crate::vfs::VfsError::NotFound,
            Ext4Error::IsDirectory => crate::vfs::VfsError::IsDirectory,
            Ext4Error::BufferTooSmall => crate::vfs::VfsError::BufferTooSmall,
            Ext4Error::InvalidPath => crate::vfs::VfsError::InvalidPath,
            _ => crate::vfs::VfsError::InvalidPath,
        })
    }

    fn metadata(&mut self, path: &str) -> Result<crate::vfs::MountMetadata, crate::vfs::VfsError> {
        let inode = self.resolve_inode(path).map_err(|error| match error {
            Ext4Error::NotFound => crate::vfs::VfsError::NotFound,
            Ext4Error::InvalidPath => crate::vfs::VfsError::InvalidPath,
            _ => crate::vfs::VfsError::InvalidPath,
        })?;
        let kind = match inode.mode & 0xf000 {
            0x4000 => crate::vfs::NodeKind::Directory,
            0x8000 => crate::vfs::NodeKind::File,
            _ => return Err(crate::vfs::VfsError::NotFound),
        };
        Ok(crate::vfs::MountMetadata {
            inode: inode.number as u64,
            kind,
            uid: inode.uid,
            gid: inode.gid,
            mode: inode.mode & 0o7777,
            size: inode.size,
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
                Ext4Error::NotFound => crate::vfs::VfsError::NotFound,
                Ext4Error::IsDirectory => crate::vfs::VfsError::IsDirectory,
                Ext4Error::BufferTooSmall => crate::vfs::VfsError::BufferTooSmall,
                Ext4Error::InvalidPath => crate::vfs::VfsError::InvalidPath,
                _ => crate::vfs::VfsError::InvalidPath,
            })
    }
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
    use std::vec;

    struct Disk(Vec<[u8; SECTOR_SIZE]>);
    impl BlockDevice for Disk {
        fn sector_count(&self) -> u64 {
            self.0.len() as u64
        }
        fn read_sector(
            &mut self,
            lba: u64,
            out: &mut [u8; SECTOR_SIZE],
        ) -> Result<(), crate::block::BlockError> {
            *out = *self
                .0
                .get(lba as usize)
                .ok_or(crate::block::BlockError::Device)?;
            Ok(())
        }
        fn write_sector(
            &mut self,
            _: u64,
            _: &[u8; SECTOR_SIZE],
        ) -> Result<(), crate::block::BlockError> {
            Err(crate::block::BlockError::Device)
        }
        fn flush(&mut self) -> Result<(), crate::block::BlockError> {
            Ok(())
        }
    }

    fn put16(bytes: &mut [u8], at: usize, value: u16) {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put32(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn fixture() -> Disk {
        let mut disk = Disk(vec![[0; SECTOR_SIZE]; 256]);
        let mut sb = [0u8; 1024];
        put32(&mut sb, 0, 32);
        put32(&mut sb, 4, 128);
        put32(&mut sb, 24, 0);
        put32(&mut sb, 32, 128);
        put32(&mut sb, 40, 32);
        put16(&mut sb, 56, EXT4_MAGIC);
        put16(&mut sb, 88, 128);
        put32(&mut sb, 96, 0x42);
        disk.0[2].copy_from_slice(&sb[..SECTOR_SIZE]);
        disk.0[3].copy_from_slice(&sb[SECTOR_SIZE..]);
        // Group 0 descriptor: inode table begins at filesystem block 5.
        put32(&mut disk.0[4], 8, 5);

        let mut root = [0u8; 128];
        put16(&mut root, 0, 0x41ed);
        put32(&mut root, 32, EXT4_EXTENTS_FL);
        put32(&mut root, 4, 1024);
        put16(&mut root, 40, 0xf30a);
        put16(&mut root, 42, 1);
        put16(&mut root, 44, 4);
        put16(&mut root, 46, 0);
        put32(&mut root, 52, 0);
        put16(&mut root, 56, 1);
        put32(&mut root, 60, 6);
        disk.0[10][128..256].copy_from_slice(&root);

        let mut file = [0u8; 128];
        put16(&mut file, 0, 0x81a4);
        put32(&mut file, 4, 5);
        put32(&mut file, 32, EXT4_EXTENTS_FL);
        put16(&mut file, 40, 0xf30a);
        put16(&mut file, 42, 1);
        put16(&mut file, 44, 4);
        put32(&mut file, 52, 0);
        put16(&mut file, 56, 1);
        put32(&mut file, 60, 7);
        disk.0[10][256..384].copy_from_slice(&file);

        let directory = &mut disk.0[12];
        put32(directory, 0, 2);
        put16(directory, 4, 12);
        directory[6] = 1;
        directory[8] = b'.';
        put32(directory, 12, 2);
        put16(directory, 16, 12);
        directory[18] = 2;
        directory[20..22].copy_from_slice(b"..");
        put32(directory, 24, 3);
        put16(directory, 28, 1000);
        directory[30] = 5;
        directory[31] = 1;
        directory[32..37].copy_from_slice(b"hello");
        disk.0[14][..5].copy_from_slice(b"world");
        disk
    }

    #[test]
    fn mounts_ext4_and_reads_a_file_through_inode_and_extent_metadata() {
        let mut volume = Ext4Volume::mount(fixture(), 0).unwrap();
        let mut output = [0u8; 8];
        assert_eq!(volume.read_path("/hello", &mut output), Ok(5));
        assert_eq!(&output[..5], b"world");
    }

    #[test]
    fn reads_holes_and_unwritten_extents_as_zeroes() {
        let mut disk = fixture();
        let inode = &mut disk.0[10][256..384];
        put32(inode, 4, 3 * 1024);
        put16(inode, 42, 2);
        put32(inode, 64, 2);
        put16(inode, 68, 32_769);
        put32(inode, 72, 8);
        disk.0[16].fill(0xff);

        let mut volume = Ext4Volume::mount(disk, 0).unwrap();
        let mut output = [0xff; 3 * 1024];
        assert_eq!(volume.read_path("/hello", &mut output), Ok(output.len()));
        assert_eq!(&output[..5], b"world");
        assert!(output[1024..].iter().all(|&byte| byte == 0));
    }

    #[test]
    fn rejects_invalid_superblocks_and_too_small_output() {
        assert_eq!(
            Ext4Volume::mount(Disk(vec![[0; SECTOR_SIZE]; 16]), 0).err(),
            Some(Ext4Error::InvalidSuperblock)
        );
        let mut volume = Ext4Volume::mount(fixture(), 0).unwrap();
        assert_eq!(
            volume.read_path("hello", &mut [0; 4]),
            Err(Ext4Error::BufferTooSmall)
        );
    }
}

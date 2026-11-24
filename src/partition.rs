//! Descoberta read-only de partições MBR/GPT e identificação inicial de ext4/FAT32.
use crate::block::{crc32, crc32_update};
use crate::block::{BlockDevice, SECTOR_SIZE};

const MAX_PARTITIONS: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filesystem {
    Ext4,
    Fat32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DetectedPartition {
    pub start_lba: u64,
    pub sectors: u64,
    pub filesystem: Filesystem,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscoveryError {
    Device,
    InvalidTable,
    UnsupportedTable,
}

/// Retorna as partições reconhecidas sem escrever no disco nem montar volumes.
pub fn discover<D: BlockDevice>(
    device: &mut D,
) -> Result<([Option<DetectedPartition>; MAX_PARTITIONS], usize), DiscoveryError> {
    let mut result = [None; MAX_PARTITIONS];
    let mut count = 0;
    let mut sector = [0u8; SECTOR_SIZE];
    device
        .read_sector(0, &mut sector)
        .map_err(|_| DiscoveryError::Device)?;
    if sector[510..512] != [0x55, 0xaa] {
        return Err(DiscoveryError::InvalidTable);
    }
    if sector[450] == 0xee {
        discover_gpt(device, &mut result, &mut count)?;
    } else {
        for slot in 0..4 {
            let offset = 446 + slot * 16;
            let start = u32le(&sector, offset + 8) as u64;
            let sectors = u32le(&sector, offset + 12) as u64;
            if start != 0 && sectors != 0 {
                detect_partition(device, start, sectors, &mut result, &mut count)?;
            }
        }
    }
    Ok((result, count))
}

fn discover_gpt<D: BlockDevice>(
    device: &mut D,
    result: &mut [Option<DetectedPartition>; MAX_PARTITIONS],
    count: &mut usize,
) -> Result<(), DiscoveryError> {
    if device.sector_count() < 2 {
        return Err(DiscoveryError::InvalidTable);
    }
    let mut header = [0u8; SECTOR_SIZE];
    device
        .read_sector(1, &mut header)
        .map_err(|_| DiscoveryError::Device)?;
    if &header[..8] != b"EFI PART" {
        return Err(DiscoveryError::InvalidTable);
    }
    let header_size = u32le(&header, 12) as usize;
    let stored_header_crc = u32le(&header, 16);
    if !(92..=SECTOR_SIZE).contains(&header_size) {
        return Err(DiscoveryError::InvalidTable);
    }
    header[16..20].fill(0);
    if crc32(&header[..header_size]) != stored_header_crc {
        return Err(DiscoveryError::InvalidTable);
    }
    let entries_lba = u64le(&header, 72);
    let entries_count = u32le(&header, 80) as usize;
    let entry_size = u32le(&header, 84) as usize;
    let entries_crc = u32le(&header, 88);
    let entry_bytes = entries_count
        .checked_mul(entry_size)
        .ok_or(DiscoveryError::InvalidTable)?;
    let entries_sectors = entry_bytes
        .checked_add(SECTOR_SIZE - 1)
        .ok_or(DiscoveryError::InvalidTable)?
        / SECTOR_SIZE;
    if entries_count == 0
        || entries_count > MAX_PARTITIONS
        || entry_size != 128
        || entries_lba
            .checked_add(entries_sectors as u64)
            .filter(|end| *end <= device.sector_count())
            .is_none()
    {
        return Err(DiscoveryError::UnsupportedTable);
    }
    let mut crc = !0u32;
    let mut sector = [0u8; SECTOR_SIZE];
    for relative in 0..entries_sectors {
        device
            .read_sector(entries_lba + relative as u64, &mut sector)
            .map_err(|_| DiscoveryError::Device)?;
        let start = relative * SECTOR_SIZE;
        let length = (entry_bytes - start).min(SECTOR_SIZE);
        crc = crc32_update(crc, &sector[..length]);
    }
    if !crc != entries_crc {
        return Err(DiscoveryError::InvalidTable);
    }
    for index in 0..entries_count {
        let byte_offset = index * entry_size;
        let lba = entries_lba + (byte_offset / SECTOR_SIZE) as u64;
        let within = byte_offset % SECTOR_SIZE;
        device
            .read_sector(lba, &mut sector)
            .map_err(|_| DiscoveryError::Device)?;
        let mut entry = [0u8; 128];
        entry.copy_from_slice(&sector[within..within + 128]);
        if entry[..16].iter().all(|byte| *byte == 0) {
            continue;
        }
        let first = u64le(&entry, 32);
        let last = u64le(&entry, 40);
        if first == 0 || last < first || last >= device.sector_count() {
            return Err(DiscoveryError::InvalidTable);
        }
        detect_partition(device, first, last - first + 1, result, count)?;
    }
    Ok(())
}

fn detect_partition<D: BlockDevice>(
    device: &mut D,
    start: u64,
    sectors: u64,
    result: &mut [Option<DetectedPartition>; MAX_PARTITIONS],
    count: &mut usize,
) -> Result<(), DiscoveryError> {
    if start
        .checked_add(sectors)
        .filter(|end| *end <= device.sector_count())
        .is_none()
    {
        return Err(DiscoveryError::InvalidTable);
    }
    let mut boot = [0u8; SECTOR_SIZE];
    device
        .read_sector(start, &mut boot)
        .map_err(|_| DiscoveryError::Device)?;
    let filesystem = if start
        .checked_add(2)
        .filter(|lba| *lba < device.sector_count())
        .is_some()
    {
        let mut superblock = [0u8; SECTOR_SIZE];
        device
            .read_sector(start + 2, &mut superblock)
            .map_err(|_| DiscoveryError::Device)?;
        if u16le(&superblock, 56) == 0xef53 {
            Some(Filesystem::Ext4)
        } else if boot[510..512] == [0x55, 0xaa] && &boot[82..90] == b"FAT32   " {
            Some(Filesystem::Fat32)
        } else {
            None
        }
    } else {
        None
    };
    if let Some(filesystem) = filesystem {
        if *count == result.len() {
            return Err(DiscoveryError::UnsupportedTable);
        }
        result[*count] = Some(DetectedPartition {
            start_lba: start,
            sectors,
            filesystem,
        });
        *count += 1;
    }
    Ok(())
}

fn u16le(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}
fn u32le(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn u64le(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

//! Redo log for a single sector overwrite, on explicitly reserved media.
//! Recovery validates the recorded target against the caller’s operation and volume.

use crate::block::{crc32, BlockDevice, BlockError, SECTOR_SIZE};

const MAGIC: &[u8; 8] = b"CARREDO1";
pub(crate) const EXT4_INODE: &[u8; 8] = b"CARE4INO";
pub(crate) const FAT32_ENTRY: &[u8; 8] = b"CARF32DE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalError {
    Block(BlockError),
    InvalidRecord,
    Pending,
}

impl From<BlockError> for JournalError {
    fn from(error: BlockError) -> Self {
        Self::Block(error)
    }
}

fn check_reservation(device: &impl BlockDevice, journal: u64) -> Result<(), JournalError> {
    if journal
        .checked_add(1)
        .is_none_or(|end| end >= device.sector_count())
    {
        return Err(BlockError::OutOfRange.into());
    }
    Ok(())
}

fn check_geometry(
    device: &impl BlockDevice,
    journal: u64,
    target: u64,
) -> Result<(), JournalError> {
    check_reservation(device, journal)?;
    if target >= device.sector_count() || target == journal || target == journal + 1 {
        return Err(BlockError::OutOfRange.into());
    }
    Ok(())
}

pub(crate) fn overwrite(
    device: &mut impl BlockDevice,
    journal: u64,
    target: u64,
    data: &[u8; SECTOR_SIZE],
) -> Result<(), JournalError> {
    overwrite_tagged(device, journal, target, data, MAGIC)
}

pub(crate) fn overwrite_tagged(
    device: &mut impl BlockDevice,
    journal: u64,
    target: u64,
    data: &[u8; SECTOR_SIZE],
    magic: &[u8; 8],
) -> Result<(), JournalError> {
    check_geometry(device, journal, target)?;
    let mut header = [0; SECTOR_SIZE];
    device.read_sector(journal, &mut header)?;
    if header != [0; SECTOR_SIZE] {
        return Err(JournalError::Pending);
    }
    header[..8].copy_from_slice(magic);
    header[8..16].copy_from_slice(&target.to_le_bytes());
    header[16..20].copy_from_slice(&crc32(data).to_le_bytes());
    let checksum = crc32(&header[..20]);
    header[20..24].copy_from_slice(&checksum.to_le_bytes());

    device.write_sector(journal + 1, data)?;
    device.flush()?;
    device.write_sector(journal, &header)?;
    device.flush()?;
    apply(device, journal, target, data)
}

pub(crate) fn recover(
    device: &mut impl BlockDevice,
    journal: u64,
    target: u64,
) -> Result<bool, JournalError> {
    check_geometry(device, journal, target)?;
    recover_tagged(device, journal, MAGIC, |lba| lba == target)
}

pub(crate) fn recover_tagged(
    device: &mut impl BlockDevice,
    journal: u64,
    magic: &[u8; 8],
    valid_target: impl FnOnce(u64) -> bool,
) -> Result<bool, JournalError> {
    check_reservation(device, journal)?;
    let mut header = [0; SECTOR_SIZE];
    device.read_sector(journal, &mut header)?;
    if header == [0; SECTOR_SIZE] {
        return Ok(false);
    }
    let mut target = [0; 8];
    target.copy_from_slice(&header[8..16]);
    let target = u64::from_le_bytes(target);
    if &header[..8] != magic
        || header[20..24] != crc32(&header[..20]).to_le_bytes()
        || header[24..].iter().any(|&byte| byte != 0)
    {
        return Err(JournalError::InvalidRecord);
    }
    if !valid_target(target) {
        return Err(JournalError::InvalidRecord);
    }
    check_geometry(device, journal, target)?;
    let mut data = [0; SECTOR_SIZE];
    device.read_sector(journal + 1, &mut data)?;
    if header[16..20] != crc32(&data).to_le_bytes() {
        return Err(JournalError::InvalidRecord);
    }
    apply(device, journal, target, &data)?;
    Ok(true)
}

fn apply(
    device: &mut impl BlockDevice,
    journal: u64,
    target: u64,
    data: &[u8; SECTOR_SIZE],
) -> Result<(), JournalError> {
    device.write_sector(target, data)?;
    device.flush()?;
    device.write_sector(journal, &[0; SECTOR_SIZE])?;
    device.flush()?;
    Ok(())
}

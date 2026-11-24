//! Circular log on an explicitly reserved block region. Each logical slot has
//! two physical copies. A completed append replaces the older copy.

use crate::block::{BlockDevice, BlockError, BlockRegion, SECTOR_SIZE};

const MAGIC: &[u8; 8] = b"CARLOG01";
const CHECKSUM_OFFSET: usize = SECTOR_SIZE - 4;
pub const MAX_RECORD: usize = CHECKSUM_OFFSET - 18;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogError {
    Block(BlockError),
    Corrupt,
    RecordTooLong,
    SequenceExhausted,
    ReopenRequired,
}

impl From<BlockError> for LogError {
    fn from(error: BlockError) -> Self {
        Self::Block(error)
    }
}

pub struct PersistentLog<D> {
    region: BlockRegion<D>,
    slots: u64,
    head: u64,
    reopen_required: bool,
}

impl<D: BlockDevice> PersistentLog<D> {
    /// Opens a reserved region initialized to zero or containing this format.
    /// Inspect both copies so a torn write cannot hide the last valid record.
    pub fn open(region: BlockRegion<D>) -> Result<Self, LogError> {
        let sectors = region.sector_count();
        if sectors < 2 || !sectors.is_multiple_of(2) {
            return Err(BlockError::OutOfRange.into());
        }
        let mut log = Self {
            region,
            slots: sectors / 2,
            head: 0,
            reopen_required: false,
        };
        for slot in 0..log.slots {
            if let Some((_, sequence, _)) = log.newest(slot)? {
                log.head = log.head.max(sequence);
            }
        }
        Ok(log)
    }

    pub fn latest_sequence(&self) -> Option<u64> {
        (self.head != 0).then_some(self.head)
    }

    pub fn append(&mut self, record: &[u8]) -> Result<u64, LogError> {
        if record.len() > MAX_RECORD {
            return Err(LogError::RecordTooLong);
        }
        if self.reopen_required {
            return Err(LogError::ReopenRequired);
        }
        let sequence = self
            .head
            .checked_add(1)
            .ok_or(LogError::SequenceExhausted)?;
        let slot = (sequence - 1) % self.slots;
        let copy = self.newest(slot)?.map_or(0, |(copy, _, _)| 1 - copy);

        let mut sector = [0; SECTOR_SIZE];
        sector[..8].copy_from_slice(MAGIC);
        sector[8..16].copy_from_slice(&sequence.to_le_bytes());
        sector[16..18].copy_from_slice(&(record.len() as u16).to_le_bytes());
        sector[18..18 + record.len()].copy_from_slice(record);
        let checksum = crate::block::crc32(&sector[..CHECKSUM_OFFSET]);
        sector[CHECKSUM_OFFSET..].copy_from_slice(&checksum.to_le_bytes());

        // A failed write or flush may have reached the disk. Recover before
        // issuing another append so we never reuse an ambiguous sequence.
        self.reopen_required = true;
        self.region.write_sector(slot * 2 + copy, &sector)?;
        self.region.flush()?;
        self.head = sequence;
        self.reopen_required = false;
        Ok(sequence)
    }

    pub fn read(
        &mut self,
        sequence: u64,
        output: &mut [u8; MAX_RECORD],
    ) -> Result<Option<usize>, LogError> {
        if self.reopen_required {
            return Err(LogError::ReopenRequired);
        }
        if sequence == 0 || sequence > self.head || self.head - sequence >= self.slots {
            return Ok(None);
        }
        let Some((_, found, sector)) = self.newest((sequence - 1) % self.slots)? else {
            return Err(LogError::Corrupt);
        };
        if sequence != found {
            return Err(LogError::Corrupt);
        }
        let len = record_len(&sector);
        output[..len].copy_from_slice(&sector[18..18 + len]);
        Ok(Some(len))
    }

    fn newest(&mut self, slot: u64) -> Result<Option<(u64, u64, [u8; SECTOR_SIZE])>, LogError> {
        let mut newest = None;
        let mut second_dirty = false;
        for copy in 0..2 {
            let mut sector = [0; SECTOR_SIZE];
            self.region.read_sector(slot * 2 + copy, &mut sector)?;
            if copy == 1 {
                second_dirty = sector.iter().any(|byte| *byte != 0);
            }
            if let Some(sequence) = decode(&sector, slot, self.slots) {
                if newest
                    .as_ref()
                    .is_none_or(|(_, previous, _)| sequence > *previous)
                {
                    newest = Some((copy, sequence, sector));
                }
            }
        }
        // One torn first append can leave a dirty copy beside a blank copy.
        // A pair with no valid records is checked against the live window on read.
        if second_dirty && newest.is_none() {
            return Err(LogError::Corrupt);
        }
        Ok(newest)
    }
}

fn record_len(sector: &[u8; SECTOR_SIZE]) -> usize {
    u16::from_le_bytes(sector[16..18].try_into().unwrap()) as usize
}

fn decode(sector: &[u8; SECTOR_SIZE], slot: u64, slots: u64) -> Option<u64> {
    let sequence = u64::from_le_bytes(sector[8..16].try_into().unwrap());
    let checksum = u32::from_le_bytes(sector[CHECKSUM_OFFSET..].try_into().unwrap());
    (sector[..8] == *MAGIC
        && sequence != 0
        && (sequence - 1) % slots == slot
        && record_len(sector) <= MAX_RECORD
        && crate::block::crc32(&sector[..CHECKSUM_OFFSET]) == checksum)
        .then_some(sequence)
}

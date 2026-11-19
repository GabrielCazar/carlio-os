//! Contrato comum de bloco usado pelos filesystems do kernel.

pub const SECTOR_SIZE: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockError {
    Device,
    OutOfRange,
    ReadOnly,
    Timeout,
    Removed,
}

/// Dispositivo lógico em setores de 512 bytes.
pub trait BlockDevice {
    fn sector_count(&self) -> u64;
    fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_SIZE]) -> Result<(), BlockError>;
    fn write_sector(&mut self, lba: u64, data: &[u8; SECTOR_SIZE]) -> Result<(), BlockError>;
    fn flush(&mut self) -> Result<(), BlockError>;
}

/// A bounded view of one partition or reserved block range. LBAs exposed by
/// this adapter are relative to `start_lba`; requests cannot escape the range.
pub struct BlockRegion<D> {
    device: D,
    start_lba: u64,
    sectors: u64,
}

impl<D: BlockDevice> BlockRegion<D> {
    pub fn new(device: D, start_lba: u64, sectors: u64) -> Result<Self, BlockError> {
        let end = start_lba
            .checked_add(sectors)
            .ok_or(BlockError::OutOfRange)?;
        if sectors == 0 || end > device.sector_count() {
            return Err(BlockError::OutOfRange);
        }
        Ok(Self {
            device,
            start_lba,
            sectors,
        })
    }

    pub fn into_inner(self) -> D {
        self.device
    }
}

impl<D: BlockDevice> BlockDevice for BlockRegion<D> {
    fn sector_count(&self) -> u64 {
        self.sectors
    }

    fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_SIZE]) -> Result<(), BlockError> {
        let physical = self
            .start_lba
            .checked_add(lba)
            .filter(|_| lba < self.sectors)
            .ok_or(BlockError::OutOfRange)?;
        self.device.read_sector(physical, out)
    }

    fn write_sector(&mut self, lba: u64, data: &[u8; SECTOR_SIZE]) -> Result<(), BlockError> {
        let physical = self
            .start_lba
            .checked_add(lba)
            .filter(|_| lba < self.sectors)
            .ok_or(BlockError::OutOfRange)?;
        self.device.write_sector(physical, data)
    }

    fn flush(&mut self) -> Result<(), BlockError> {
        self.device.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MemoryDisk {
        sectors: Vec<[u8; SECTOR_SIZE]>,
        last_write: Option<u64>,
    }

    impl BlockDevice for MemoryDisk {
        fn sector_count(&self) -> u64 {
            self.sectors.len() as u64
        }
        fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_SIZE]) -> Result<(), BlockError> {
            *out = *self
                .sectors
                .get(lba as usize)
                .ok_or(BlockError::OutOfRange)?;
            Ok(())
        }
        fn write_sector(&mut self, lba: u64, data: &[u8; SECTOR_SIZE]) -> Result<(), BlockError> {
            *self
                .sectors
                .get_mut(lba as usize)
                .ok_or(BlockError::OutOfRange)? = *data;
            self.last_write = Some(lba);
            Ok(())
        }
        fn flush(&mut self) -> Result<(), BlockError> {
            Ok(())
        }
    }

    #[test]
    fn region_translates_lbas_and_rejects_reads_and_writes_outside() {
        let disk = MemoryDisk {
            sectors: vec![[0; SECTOR_SIZE]; 16],
            last_write: None,
        };
        let mut region = BlockRegion::new(disk, 4, 8).unwrap();
        region.write_sector(7, &[0x5a; SECTOR_SIZE]).unwrap();
        assert_eq!(region.device.last_write, Some(11));
        assert_eq!(
            region.read_sector(8, &mut [0; SECTOR_SIZE]),
            Err(BlockError::OutOfRange)
        );
        assert_eq!(
            region.write_sector(u64::MAX, &[0; SECTOR_SIZE]),
            Err(BlockError::OutOfRange)
        );
        assert_eq!(
            BlockRegion::new(region.into_inner(), u64::MAX, 2).err(),
            Some(BlockError::OutOfRange)
        );
    }
}

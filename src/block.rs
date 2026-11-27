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
    /// Lê setores consecutivos em um buffer alinhado em tamanho de setor.
    /// Valida o intervalo inteiro antes de emitir a primeira leitura.
    fn read_sectors(&mut self, lba: u64, out: &mut [u8]) -> Result<(), BlockError> {
        checked_sector_range(lba, out.len(), self.sector_count())?;
        for (offset, sector) in out.as_chunks_mut::<SECTOR_SIZE>().0.iter_mut().enumerate() {
            self.read_sector(lba + offset as u64, sector)?;
        }
        Ok(())
    }
    fn write_sector(&mut self, lba: u64, data: &[u8; SECTOR_SIZE]) -> Result<(), BlockError>;
    /// Validate the entire range before issuing writes. A device error can
    /// leave a prefix written. Callers control durability through `flush`.
    fn write_sectors(&mut self, lba: u64, data: &[u8]) -> Result<(), BlockError> {
        checked_sector_range(lba, data.len(), self.sector_count())?;
        for (offset, sector) in data.as_chunks::<SECTOR_SIZE>().0.iter().enumerate() {
            self.write_sector(lba + offset as u64, sector)?;
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<(), BlockError>;
}

fn checked_sector_range(lba: u64, bytes: usize, device_sectors: u64) -> Result<(), BlockError> {
    let sectors = bytes / SECTOR_SIZE;
    if !bytes.is_multiple_of(SECTOR_SIZE)
        || lba
            .checked_add(sectors as u64)
            .is_none_or(|end| end > device_sectors)
    {
        return Err(BlockError::OutOfRange);
    }
    Ok(())
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

    fn read_sectors(&mut self, lba: u64, out: &mut [u8]) -> Result<(), BlockError> {
        checked_sector_range(lba, out.len(), self.sectors)?;
        self.device.read_sectors(self.start_lba + lba, out)
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

    fn write_sectors(&mut self, lba: u64, data: &[u8]) -> Result<(), BlockError> {
        checked_sector_range(lba, data.len(), self.sectors)?;
        self.device.write_sectors(self.start_lba + lba, data)
    }
}

pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    !crc32_update(!0, bytes)
}
pub(crate) fn crc32_update(mut crc: u32, bytes: &[u8]) -> u32 {
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    crc
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
        let mut sectors = [0; SECTOR_SIZE * 2];
        region.read_sectors(6, &mut sectors).unwrap();
        assert_eq!(sectors[..SECTOR_SIZE], [0; SECTOR_SIZE]);
        assert_eq!(sectors[SECTOR_SIZE..], [0x5a; SECTOR_SIZE]);
        assert_eq!(
            region.read_sectors(7, &mut sectors),
            Err(BlockError::OutOfRange)
        );
        assert_eq!(sectors[..SECTOR_SIZE], [0; SECTOR_SIZE]);
        assert_eq!(
            region.read_sector(8, &mut [0; SECTOR_SIZE]),
            Err(BlockError::OutOfRange)
        );
        assert_eq!(
            region.write_sector(u64::MAX, &[0; SECTOR_SIZE]),
            Err(BlockError::OutOfRange)
        );
        sectors.fill(0x6b);
        assert_eq!(
            region.write_sectors(7, &sectors),
            Err(BlockError::OutOfRange)
        );
        assert_eq!(
            region.write_sectors(6, &sectors[..513]),
            Err(BlockError::OutOfRange)
        );
        assert_eq!(region.device.sectors[11], [0x5a; SECTOR_SIZE]);
        region.write_sectors(6, &sectors).unwrap();
        assert_eq!(region.device.sectors[10], [0x6b; SECTOR_SIZE]);
        assert_eq!(region.device.sectors[11], [0x6b; SECTOR_SIZE]);
        assert_eq!(region.device.sectors[12], [0; SECTOR_SIZE]);
        assert_eq!(
            BlockRegion::new(region.into_inner(), u64::MAX, 2).err(),
            Some(BlockError::OutOfRange)
        );
    }
}

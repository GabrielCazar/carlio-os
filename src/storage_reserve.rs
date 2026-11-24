//! Layout for a dedicated, explicitly provisioned CarlioOS persistence disk.

use crate::block::{BlockDevice, BlockError, BlockRegion, SECTOR_SIZE};

pub const LOG_START: u64 = 2064;
pub const LOG_SECTORS: u64 = 128;
pub const SWAP_START: u64 = 4096;
const _: () = assert!(LOG_START + LOG_SECTORS <= SWAP_START);
const VFS_START: u64 = SWAP_START;
const VFS_SECTORS: u64 = 4096;
const VFS_MAGIC: &[u8] = b"CARLIO-PERSIST-02";
const MAGIC: &[u8] = b"CARLIO-PERSIST-01";

#[derive(Clone, Copy)]
pub struct StorageReserve {
    sectors: u64,
    swap_start: u64,
}

impl StorageReserve {
    /// Sector zero must contain the exact version marker followed by zeros.
    /// The marker provisions the entire device and excludes MBR/GPT layouts.
    pub fn detect(device: &mut impl BlockDevice) -> Result<Option<Self>, BlockError> {
        let sectors = device.sector_count();
        if sectors < SWAP_START + 8 {
            return Ok(None);
        }
        let mut header = [0; SECTOR_SIZE];
        device.read_sector(0, &mut header)?;
        let matches = |magic: &[u8]| {
            header.starts_with(magic) && header[magic.len()..].iter().all(|&byte| byte == 0)
        };
        let swap_start = if matches(VFS_MAGIC) {
            VFS_START + VFS_SECTORS
        } else if matches(MAGIC) {
            SWAP_START
        } else {
            return Ok(None);
        };
        Ok((sectors >= swap_start + 8).then_some(Self {
            sectors,
            swap_start,
        }))
    }

    pub fn has_vfs(&self) -> bool {
        self.swap_start != SWAP_START
    }

    pub fn vfs<D: BlockDevice>(&self, device: D) -> Result<BlockRegion<D>, BlockError> {
        if !self.has_vfs() {
            return Err(BlockError::OutOfRange);
        }
        self.region(device, VFS_START, VFS_SECTORS)
    }

    pub fn log<D: BlockDevice>(&self, device: D) -> Result<BlockRegion<D>, BlockError> {
        self.region(device, LOG_START, LOG_SECTORS)
    }

    pub fn swap<D: BlockDevice>(&self, device: D) -> Result<BlockRegion<D>, BlockError> {
        self.region(
            device,
            self.swap_start,
            (self.sectors - self.swap_start) / 8 * 8,
        )
    }

    fn region<D: BlockDevice>(
        &self,
        device: D,
        start: u64,
        sectors: u64,
    ) -> Result<BlockRegion<D>, BlockError> {
        if device.sector_count() != self.sectors {
            return Err(BlockError::OutOfRange);
        }
        BlockRegion::new(device, start, sectors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Disk([u8; SECTOR_SIZE]);

    impl BlockDevice for Disk {
        fn sector_count(&self) -> u64 {
            SWAP_START + 17
        }

        fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_SIZE]) -> Result<(), BlockError> {
            assert_eq!(lba, 0);
            *out = self.0;
            Ok(())
        }

        fn write_sector(&mut self, _: u64, _: &[u8; SECTOR_SIZE]) -> Result<(), BlockError> {
            panic!("discovery must be read-only");
        }

        fn flush(&mut self) -> Result<(), BlockError> {
            panic!("discovery must be read-only");
        }
    }

    #[test]
    fn only_dedicated_media_expose_disjoint_log_and_swap_regions() {
        let mut disk = Disk([0; SECTOR_SIZE]);
        assert!(StorageReserve::detect(&mut disk).unwrap().is_none());
        disk.0[..MAGIC.len()].copy_from_slice(MAGIC);
        disk.0[510..].copy_from_slice(&[0x55, 0xaa]);
        assert!(StorageReserve::detect(&mut disk).unwrap().is_none());
        disk.0[510..].fill(0);
        let reserve = StorageReserve::detect(&mut disk).unwrap().unwrap();
        assert_eq!(
            reserve.log(Disk([0; SECTOR_SIZE])).unwrap().sector_count(),
            LOG_SECTORS
        );
        assert_eq!(reserve.swap(disk).unwrap().sector_count(), 16);
    }
}

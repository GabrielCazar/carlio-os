//! Layout for a dedicated, explicitly provisioned CarlioOS persistence disk.

use crate::block::{BlockDevice, BlockError, BlockRegion, SECTOR_SIZE};

pub const LOG_START: u64 = 2064;
pub const LOG_SECTORS: u64 = 128;
pub const SWAP_START: u64 = 4096;
const _: () = assert!(LOG_START + LOG_SECTORS <= SWAP_START);
const VFS_START: u64 = SWAP_START;
const VFS_SECTORS: u64 = 4096;
const VFS_MAGIC: &[u8] = b"CARLIO-PERSIST-02";
const CONFIG_MAGIC: &[u8] = b"CARLIO-PERSIST-03";
pub const MAX_SWAP_PAGES: u64 = 64;
const SECTORS_PER_PAGE: u64 = 4096 / SECTOR_SIZE as u64;
const MAGIC: &[u8] = b"CARLIO-PERSIST-01";

#[derive(Clone, Copy)]
pub struct StorageReserve {
    sectors: u64,
    swap_start: u64,
    swap_sectors: u64,
}

impl StorageReserve {
    /// Sector zero must contain a recognized version and valid layout fields.
    /// The marker provisions the entire device and excludes MBR/GPT layouts.
    pub fn detect(device: &mut impl BlockDevice) -> Result<Option<Self>, BlockError> {
        let sectors = device.sector_count();
        if sectors < SWAP_START {
            return Ok(None);
        }
        let mut header = [0; SECTOR_SIZE];
        device.read_sector(0, &mut header)?;
        let matches = |magic: &[u8]| {
            header.starts_with(magic) && header[magic.len()..].iter().all(|&byte| byte == 0)
        };
        let (swap_start, swap_sectors) = if header.starts_with(CONFIG_MAGIC) {
            let start = match header[17] {
                0 => SWAP_START,
                1 => VFS_START + VFS_SECTORS,
                _ => return Ok(None),
            };
            let pages = u64::from(header[18]);
            if pages > MAX_SWAP_PAGES || header[19..].iter().any(|&byte| byte != 0) {
                return Ok(None);
            }
            (start, pages * SECTORS_PER_PAGE)
        } else {
            let start = if matches(VFS_MAGIC) {
                VFS_START + VFS_SECTORS
            } else if matches(MAGIC) {
                SWAP_START
            } else {
                return Ok(None);
            };
            if sectors < start + SECTORS_PER_PAGE {
                return Ok(None);
            }
            let pages = ((sectors - start) / SECTORS_PER_PAGE).min(MAX_SWAP_PAGES);
            (start, pages * SECTORS_PER_PAGE)
        };
        Ok((sectors >= swap_start + swap_sectors).then_some(Self {
            sectors,
            swap_start,
            swap_sectors,
        }))
    }

    pub fn has_swap(&self) -> bool {
        self.swap_sectors != 0
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
        if !self.has_swap() {
            return Err(BlockError::OutOfRange);
        }
        self.region(device, self.swap_start, self.swap_sectors)
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

    struct Disk([u8; SECTOR_SIZE], u64);

    impl BlockDevice for Disk {
        fn sector_count(&self) -> u64 {
            self.1
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
        let mut disk = Disk([0; SECTOR_SIZE], SWAP_START + 17);
        assert!(StorageReserve::detect(&mut disk).unwrap().is_none());
        disk.0[..MAGIC.len()].copy_from_slice(MAGIC);
        disk.0[510..].copy_from_slice(&[0x55, 0xaa]);
        assert!(StorageReserve::detect(&mut disk).unwrap().is_none());
        disk.0[510..].fill(0);
        let reserve = StorageReserve::detect(&mut disk).unwrap().unwrap();
        assert_eq!(
            reserve
                .log(Disk([0; SECTOR_SIZE], SWAP_START + 17))
                .unwrap()
                .sector_count(),
            LOG_SECTORS
        );
        assert_eq!(reserve.swap(disk).unwrap().sector_count(), 16);
    }
    #[test]
    fn configured_layout_bounds_swap_and_can_disable_it() {
        let start = VFS_START + VFS_SECTORS;
        let mut disk = Disk([0; SECTOR_SIZE], start);
        disk.0[..CONFIG_MAGIC.len()].copy_from_slice(CONFIG_MAGIC);
        disk.0[17] = 1;
        let reserve = StorageReserve::detect(&mut disk).unwrap().unwrap();
        assert!(reserve.has_vfs());
        assert!(!reserve.has_swap());

        disk.0[18] = 2;
        assert!(StorageReserve::detect(&mut disk).unwrap().is_none());
        disk.1 += 128;
        let reserve = StorageReserve::detect(&mut disk).unwrap().unwrap();
        assert_eq!(reserve.swap_sectors, 2 * SECTORS_PER_PAGE);

        disk.0[18] = MAX_SWAP_PAGES as u8 + 1;
        disk.1 += 1024;
        assert!(StorageReserve::detect(&mut disk).unwrap().is_none());
        disk.0[18] = 1;
        disk.0[510..].copy_from_slice(&[0x55, 0xaa]);
        assert!(StorageReserve::detect(&mut disk).unwrap().is_none());
    }
}

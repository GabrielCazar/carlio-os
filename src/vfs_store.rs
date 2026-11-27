//! Two independently checksummed banks for the CarlioOS VFS overlay.
use crate::block::{BlockDevice, BlockError, SECTOR_SIZE, crc32};

const MAGIC: &[u8; 8] = b"CARVFSB1";

pub struct VfsStore<D> {
    device: D,
    bank_sectors: u64,
    active: Option<(usize, u64)>,
    recovery_only: bool,
}

impl<D: BlockDevice> VfsStore<D> {
    /// Opens the newest readable snapshot. A fallback is for recovery only
    /// because it can contain superseded credentials and file permissions.
    pub fn open(mut device: D, output: &mut [u8]) -> Result<(Self, usize), BlockError> {
        let bank_sectors = device.sector_count() / 2;
        if bank_sectors <= output.len().div_ceil(SECTOR_SIZE) as u64 {
            return Err(BlockError::OutOfRange);
        }
        let mut headers = [[0; SECTOR_SIZE]; 2];
        let mut read_error = None;
        let mut records = [None; 2];
        let mut empty = true;
        let mut recovery_only = false;
        for (bank, header) in headers.iter_mut().enumerate() {
            match device.read_sector(bank as u64 * bank_sectors, header) {
                Ok(()) if *header == [0; SECTOR_SIZE] => {}
                Ok(()) => {
                    empty = false;
                    records[bank] = parse_header(header);
                    recovery_only |= records[bank].is_none();
                }
                Err(error) => {
                    read_error.get_or_insert(error);
                    recovery_only = true;
                    empty = false;
                }
            }
        }
        let mut store = Self {
            device,
            bank_sectors,
            active: None,
            recovery_only,
        };
        let mut order = [0, 1];
        if records[1].map(|r| r.0) > records[0].map(|r| r.0) {
            order.swap(0, 1);
        }
        'banks: for bank in order {
            let Some((sequence, length, checksum)) = records[bank] else {
                continue;
            };
            if length == 0 || length > output.len() {
                store.recovery_only = true;
                continue;
            }
            let mut sector = [0; SECTOR_SIZE];
            for (index, chunk) in output[..length].chunks_mut(SECTOR_SIZE).enumerate() {
                if let Err(error) = store
                    .device
                    .read_sector(bank as u64 * bank_sectors + 1 + index as u64, &mut sector)
                {
                    read_error.get_or_insert(error);
                    store.recovery_only = true;
                    continue 'banks;
                }
                chunk.copy_from_slice(&sector[..chunk.len()]);
            }
            if crc32(&output[..length]) == checksum {
                store.active = Some((bank, sequence));
                return Ok((store, length));
            }
            store.recovery_only = true;
        }
        if empty {
            Ok((store, 0))
        } else {
            Err(read_error.unwrap_or(BlockError::Device))
        }
    }

    /// Indicates that opening the overlay could have rolled back committed data.
    pub fn recovery_only(&self) -> bool {
        self.recovery_only
    }

    pub fn commit(&mut self, data: &[u8]) -> Result<(), BlockError> {
        if self.recovery_only {
            return Err(BlockError::ReadOnly);
        }
        if data.is_empty() || data.len().div_ceil(SECTOR_SIZE) as u64 >= self.bank_sectors {
            return Err(BlockError::OutOfRange);
        }
        let length = u32::try_from(data.len()).map_err(|_| BlockError::OutOfRange)?;
        let (bank, previous) = self
            .active
            .map_or((0, 0), |(bank, sequence)| (1 - bank, sequence));
        let sequence = previous.checked_add(1).ok_or(BlockError::OutOfRange)?;
        let start = bank as u64 * self.bank_sectors;
        let mut sector = [0; SECTOR_SIZE];
        for (index, chunk) in data.chunks(SECTOR_SIZE).enumerate() {
            sector.fill(0);
            sector[..chunk.len()].copy_from_slice(chunk);
            self.device
                .write_sector(start + 1 + index as u64, &sector)?;
        }
        self.device.flush()?;
        sector.fill(0);
        sector[..8].copy_from_slice(MAGIC);
        sector[8..16].copy_from_slice(&sequence.to_le_bytes());
        sector[16..20].copy_from_slice(&length.to_le_bytes());
        sector[20..24].copy_from_slice(&crc32(data).to_le_bytes());
        let checksum = crc32(&sector[..24]);
        sector[24..28].copy_from_slice(&checksum.to_le_bytes());
        self.device.write_sector(start, &sector)?;
        self.device.flush()?;
        self.active = Some((bank, sequence));
        Ok(())
    }
}

fn parse_header(header: &[u8; SECTOR_SIZE]) -> Option<(u64, usize, u32)> {
    if &header[..8] != MAGIC
        || header[24..28] != crc32(&header[..24]).to_le_bytes()
        || header[28..].iter().any(|&byte| byte != 0)
    {
        return None;
    }
    let sequence = u64::from_le_bytes(header[8..16].try_into().ok()?);
    let length = u32::from_le_bytes(header[16..20].try_into().ok()?);
    let checksum = u32::from_le_bytes(header[20..24].try_into().ok()?);
    (sequence != 0).then_some((sequence, length as usize, checksum))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BANK_SECTORS: usize = 4;
    type Disk = [[u8; SECTOR_SIZE]; 2 * BANK_SECTORS];

    /// Models a write cache discarded on power loss and a failing I/O operation.
    struct TestDevice {
        durable: Disk,
        cached: Disk,
        unreadable: Option<u64>,
        operations_before_failure: Option<usize>,
    }

    impl Default for TestDevice {
        fn default() -> Self {
            Self {
                durable: [[0; SECTOR_SIZE]; 2 * BANK_SECTORS],
                cached: [[0; SECTOR_SIZE]; 2 * BANK_SECTORS],
                unreadable: None,
                operations_before_failure: None,
            }
        }
    }

    impl TestDevice {
        fn checkpoint(&mut self) -> Result<(), BlockError> {
            if let Some(remaining) = &mut self.operations_before_failure {
                *remaining = remaining.checked_sub(1).ok_or(BlockError::Timeout)?;
            }
            Ok(())
        }
    }

    impl BlockDevice for TestDevice {
        fn sector_count(&self) -> u64 {
            self.durable.len() as u64
        }

        fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_SIZE]) -> Result<(), BlockError> {
            if self.unreadable == Some(lba) {
                return Err(BlockError::Timeout);
            }
            *out = self.cached[lba as usize];
            Ok(())
        }

        fn write_sector(&mut self, lba: u64, data: &[u8; SECTOR_SIZE]) -> Result<(), BlockError> {
            self.checkpoint()?;
            self.cached[lba as usize] = *data;
            Ok(())
        }

        fn flush(&mut self) -> Result<(), BlockError> {
            self.checkpoint()?;
            self.durable = self.cached;
            Ok(())
        }
    }

    fn two_snapshots() -> TestDevice {
        let (mut store, len) = VfsStore::open(TestDevice::default(), &mut [0; 1024]).unwrap();
        assert_eq!(len, 0);
        store.commit(b"previous").unwrap();
        store.commit(b"current").unwrap();
        store.device
    }

    #[test]
    fn io_failure_or_corruption_recovers_previous_snapshot_read_only() {
        for (unreadable, corrupt) in [
            (Some(BANK_SECTORS as u64), None),
            (Some(BANK_SECTORS as u64 + 1), None),
            (None, Some(BANK_SECTORS)),
            (None, Some(BANK_SECTORS + 1)),
        ] {
            let mut device = two_snapshots();
            device.unreadable = unreadable;
            if let Some(sector) = corrupt {
                device.cached[sector][0] ^= 0xff;
            }
            let mut output = [0; 1024];
            let (mut store, len) = VfsStore::open(device, &mut output).unwrap();
            assert_eq!(&output[..len], b"previous");
            assert!(store.recovery_only());
            assert_eq!(store.commit(b"replacement"), Err(BlockError::ReadOnly));
        }
    }

    #[test]
    fn unreadable_empty_header_cannot_initialize_the_overlay() {
        let device = TestDevice {
            unreadable: Some(0),
            ..TestDevice::default()
        };
        assert!(matches!(
            VfsStore::open(device, &mut [0; 1024]),
            Err(BlockError::Timeout)
        ));
    }

    #[test]
    fn interrupted_commit_preserves_latest_durable_snapshot() {
        let replacement = [0x55; SECTOR_SIZE + 1];
        // Two data writes, data flush, header write and header flush.
        for completed_operations in 0..=5 {
            let mut output = [0; 1024];
            let (mut store, _) = VfsStore::open(two_snapshots(), &mut output).unwrap();
            store.device.operations_before_failure = Some(completed_operations);
            let commit = store.commit(&replacement);
            let mut device = store.device;
            device.cached = device.durable;
            let (reopened, len) = VfsStore::open(device, &mut output).unwrap();
            assert!(!reopened.recovery_only());
            if completed_operations == 5 {
                assert_eq!(commit, Ok(()));
                assert_eq!(&output[..len], &replacement);
            } else {
                assert_eq!(commit, Err(BlockError::Timeout));
                assert_eq!(&output[..len], b"current");
            }
        }
    }
}

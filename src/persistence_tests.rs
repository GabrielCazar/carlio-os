use crate::{
    block::{BlockDevice, BlockError, BlockRegion, SECTOR_SIZE},
    persistent_log::{LogError, PersistentLog, MAX_RECORD},
    swap::{SwapError, SwapSlot, SwapStore, PAGE_SIZE},
};

struct Disk {
    sectors: [[u8; SECTOR_SIZE]; 8],
    tear: bool,
    fail_flush: bool,
}

impl Default for Disk {
    fn default() -> Self {
        Self {
            sectors: [[0; SECTOR_SIZE]; 8],
            tear: false,
            fail_flush: false,
        }
    }
}

impl BlockDevice for &mut Disk {
    fn sector_count(&self) -> u64 {
        self.sectors.len() as u64
    }

    fn read_sector(&mut self, lba: u64, output: &mut [u8; SECTOR_SIZE]) -> Result<(), BlockError> {
        *output = self.sectors[lba as usize];
        Ok(())
    }

    fn write_sector(&mut self, lba: u64, input: &[u8; SECTOR_SIZE]) -> Result<(), BlockError> {
        let target = &mut self.sectors[lba as usize];
        if self.tear {
            target[..100].copy_from_slice(&input[..100]);
            return Err(BlockError::Removed);
        }
        *target = *input;
        Ok(())
    }

    fn flush(&mut self) -> Result<(), BlockError> {
        if self.fail_flush {
            return Err(BlockError::Timeout);
        }
        Ok(())
    }
}

#[test]
fn log_recovers_torn_sector_and_reopens_after_ambiguous_flush() {
    let mut disk = Disk {
        tear: true,
        ..Disk::default()
    };
    let mut log = PersistentLog::open(BlockRegion::new(&mut disk, 0, 2).unwrap()).unwrap();
    assert_eq!(
        log.append(b"initial torn"),
        Err(LogError::Block(BlockError::Removed))
    );
    assert_eq!(log.append(b"retry"), Err(LogError::ReopenRequired));

    disk.tear = false;
    let mut log = PersistentLog::open(BlockRegion::new(&mut disk, 0, 2).unwrap()).unwrap();
    assert_eq!(log.append(b"stable"), Ok(1));

    disk.tear = true;
    let mut log = PersistentLog::open(BlockRegion::new(&mut disk, 0, 2).unwrap()).unwrap();
    assert_eq!(
        log.append(b"torn"),
        Err(LogError::Block(BlockError::Removed))
    );
    disk.tear = false;
    disk.fail_flush = true;
    let mut log = PersistentLog::open(BlockRegion::new(&mut disk, 0, 2).unwrap()).unwrap();
    let mut output = [0; MAX_RECORD];
    assert_eq!(log.read(1, &mut output), Ok(Some(6)));
    assert_eq!(&output[..6], b"stable");
    assert_eq!(
        log.append(b"ambiguous"),
        Err(LogError::Block(BlockError::Timeout))
    );

    disk.fail_flush = false;
    let mut log = PersistentLog::open(BlockRegion::new(&mut disk, 0, 2).unwrap()).unwrap();
    assert_eq!(log.latest_sequence(), Some(2));
    assert_eq!(log.read(1, &mut output), Ok(None));
    assert_eq!(log.read(2, &mut output), Ok(Some(9)));
    assert_eq!(&output[..9], b"ambiguous");
    disk.sectors[0][20] ^= 1;
    disk.sectors[1][20] ^= 1;
    assert!(matches!(
        PersistentLog::open(BlockRegion::new(&mut disk, 0, 2).unwrap()),
        Err(LogError::Corrupt)
    ));
}

#[test]
fn swap_checks_owner_capacity_and_reused_handles() {
    let mut disk = Disk::default();
    let mut slots = [SwapSlot::default()];
    let mut store = SwapStore::new(
        BlockRegion::new(&mut disk, 0, 8).unwrap(),
        zeroize::Zeroizing::new([0x53; 32]),
        [1; 16],
        &mut slots,
    )
    .unwrap();
    let page = [0x6a; PAGE_SIZE];
    let mut output = [0xff; PAGE_SIZE];
    let first = store.page_out(9, &page).unwrap();
    assert_eq!(store.page_out(9, &page), Err(SwapError::Full));
    assert_eq!(
        store.page_in(10, &first, &mut output),
        Err(SwapError::InvalidPage)
    );
    assert_eq!(output, [0; PAGE_SIZE]);
    assert_eq!(store.page_in(9, &first, &mut output), Ok(()));
    assert_eq!(output, page);
    store.release(9, &first).unwrap();
    let second = store.page_out(9, &page).unwrap();
    assert_ne!(first, second);
    assert_eq!(store.release(9, &first), Err(SwapError::InvalidPage));
}

#[test]
fn swap_retries_failed_flush_and_rejects_changed_ciphertext() {
    use std::cell::RefCell;

    let disk = RefCell::new(Disk {
        fail_flush: true,
        ..Disk::default()
    });
    struct Shared<'a>(&'a RefCell<Disk>);
    impl BlockDevice for Shared<'_> {
        fn sector_count(&self) -> u64 {
            8
        }
        fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_SIZE]) -> Result<(), BlockError> {
            (&mut *self.0.borrow_mut()).read_sector(lba, out)
        }
        fn write_sector(&mut self, lba: u64, data: &[u8; SECTOR_SIZE]) -> Result<(), BlockError> {
            (&mut *self.0.borrow_mut()).write_sector(lba, data)
        }
        fn flush(&mut self) -> Result<(), BlockError> {
            (&mut *self.0.borrow_mut()).flush()
        }
    }
    let mut slots = [SwapSlot::default()];
    let mut store = SwapStore::new(
        BlockRegion::new(Shared(&disk), 0, 8).unwrap(),
        zeroize::Zeroizing::new([0x53; 32]),
        [1; 16],
        &mut slots,
    )
    .unwrap();
    let page = [0x40; PAGE_SIZE];
    assert_eq!(
        store.page_out(42, &page),
        Err(SwapError::Block(BlockError::Timeout))
    );
    disk.borrow_mut().fail_flush = false;
    let handle = store.page_out(42, &page).unwrap();
    assert_ne!(&disk.borrow().sectors[0], &page[..SECTOR_SIZE]);
    disk.borrow_mut().sectors[7][511] ^= 1;
    let mut output = [0xff; PAGE_SIZE];
    assert_eq!(
        store.page_in(42, &handle, &mut output),
        Err(SwapError::Authentication)
    );
    assert_eq!(output, [0; PAGE_SIZE]);
}

#[test]
fn metadata_redo_rejects_the_wrong_type_target_and_checksum() {
    use crate::block::crc32;
    use crate::sector_journal::{recover_tagged, JournalError, EXT4_INODE, FAT32_ENTRY};

    let mut disk = Disk::default();
    let data = [0x61; SECTOR_SIZE];
    let header = &mut disk.sectors[1];
    header[..8].copy_from_slice(FAT32_ENTRY);
    header[8..16].copy_from_slice(&4u64.to_le_bytes());
    header[16..20].copy_from_slice(&crc32(&data).to_le_bytes());
    let crc = crc32(&header[..20]);
    header[20..24].copy_from_slice(&crc.to_le_bytes());
    disk.sectors[2] = data;

    assert_eq!(
        recover_tagged(&mut &mut disk, 1, EXT4_INODE, |_| true),
        Err(JournalError::InvalidRecord)
    );
    assert_eq!(
        recover_tagged(&mut &mut disk, 1, FAT32_ENTRY, |lba| lba >= 5),
        Err(JournalError::InvalidRecord)
    );
    disk.sectors[2][1] ^= 1;
    assert_eq!(
        recover_tagged(&mut &mut disk, 1, FAT32_ENTRY, |lba| lba == 4),
        Err(JournalError::InvalidRecord)
    );
    assert_eq!(disk.sectors[4], [0; SECTOR_SIZE]);
    assert_eq!(&disk.sectors[1][..8], FAT32_ENTRY);
    disk.sectors[2] = data;
    assert_eq!(
        recover_tagged(&mut &mut disk, 1, FAT32_ENTRY, |lba| lba == 4),
        Ok(true)
    );
    assert_eq!(disk.sectors[4], data);
    assert_eq!(disk.sectors[1], [0; SECTOR_SIZE]);
}

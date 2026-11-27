use carlio_os::{
    block::{BlockDevice, BlockError, SECTOR_SIZE},
    storage_reserve::StorageReserve,
    vfs::{AccessContext, Credentials, Vfs},
    vfs_store::VfsStore,
};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
};
static OVERLAY: std::sync::Mutex<Vfs<40>> = std::sync::Mutex::new(Vfs::new());
struct Image(File, u64);

impl BlockDevice for Image {
    fn sector_count(&self) -> u64 {
        self.1
    }

    fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_SIZE]) -> Result<(), BlockError> {
        self.0
            .seek(SeekFrom::Start(lba * SECTOR_SIZE as u64))
            .and_then(|_| self.0.read_exact(out))
            .map_err(|_| BlockError::Device)
    }

    fn write_sector(&mut self, lba: u64, data: &[u8; SECTOR_SIZE]) -> Result<(), BlockError> {
        self.0
            .seek(SeekFrom::Start(lba * SECTOR_SIZE as u64))
            .and_then(|_| self.0.write_all(data))
            .map_err(|_| BlockError::Device)
    }

    fn flush(&mut self) -> Result<(), BlockError> {
        self.0.sync_all().map_err(|_| BlockError::Device)
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&args[1])
        .unwrap();
    let sectors = file.metadata().unwrap().len() / SECTOR_SIZE as u64;
    let mut image = Image(file, sectors);
    let reserve = StorageReserve::detect(&mut image).unwrap().unwrap();
    let region = reserve.vfs(image).unwrap();
    let mut snapshot = vec![0; Vfs::<40>::SNAPSHOT_MAX];
    let (mut store, size) = VfsStore::open(region, &mut snapshot).unwrap();
    let mut vfs = OVERLAY.lock().unwrap();
    vfs.restore_snapshot(&snapshot[..size]).unwrap();
    let root = Credentials { uid: 0, gid: 0 };
    vfs.create_file("/", "service-tcp", root, 0o755).unwrap();
    let data = fs::read(&args[2]).unwrap();
    vfs.write_file_as("/service-tcp", AccessContext::new(root, &[], 0), &data)
        .unwrap();
    let size = vfs.encode_snapshot(&mut snapshot).unwrap();
    store.commit(&snapshot[..size]).unwrap();
}

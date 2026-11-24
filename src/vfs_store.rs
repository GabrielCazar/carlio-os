//! Two independently checksummed banks for the CarlioOS VFS overlay.
use crate::block::{crc32, BlockDevice, BlockError, SECTOR_SIZE};

const MAGIC: &[u8; 8] = b"CARVFSB1";

pub struct VfsStore<D> {
    device: D,
    bank_sectors: u64,
    active: Option<(usize, u64)>,
}

impl<D: BlockDevice> VfsStore<D> {
    pub fn open(mut device: D, output: &mut [u8]) -> Result<(Self, usize), BlockError> {
        let bank_sectors = device.sector_count() / 2;
        if bank_sectors <= output.len().div_ceil(SECTOR_SIZE) as u64 {
            return Err(BlockError::OutOfRange);
        }
        let mut headers = [[0; SECTOR_SIZE]; 2];
        for (bank, header) in headers.iter_mut().enumerate() {
            device.read_sector(bank as u64 * bank_sectors, header)?;
        }
        let mut store = Self {
            device,
            bank_sectors,
            active: None,
        };
        let records = headers.each_ref().map(parse_header);
        let mut order = [0, 1];
        if records[1].map(|r| r.0) > records[0].map(|r| r.0) {
            order.swap(0, 1);
        }
        for bank in order {
            let Some((sequence, length, checksum)) = records[bank] else {
                continue;
            };
            if length == 0 || length > output.len() {
                continue;
            }
            let mut sector = [0; SECTOR_SIZE];
            for (index, chunk) in output[..length].chunks_mut(SECTOR_SIZE).enumerate() {
                store
                    .device
                    .read_sector(bank as u64 * bank_sectors + 1 + index as u64, &mut sector)?;
                chunk.copy_from_slice(&sector[..chunk.len()]);
            }
            if crc32(&output[..length]) == checksum {
                store.active = Some((bank, sequence));
                return Ok((store, length));
            }
        }
        if headers.iter().all(|header| *header == [0; SECTOR_SIZE]) {
            Ok((store, 0))
        } else {
            Err(BlockError::Device)
        }
    }

    pub fn commit(&mut self, data: &[u8]) -> Result<(), BlockError> {
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

//! Protótipo em memória do núcleo copy-on-write do Hermione.
//!
//! Não é um formato em disco nem fornece criptografia. CRC32 detecta erros
//! acidentais nos testes, mas não autentica dados contra adulteração.

pub const BLOCK_SIZE: usize = 512;
const LOGICAL_BLOCKS: usize = 16;
const PHYSICAL_BLOCKS: usize = 64;
const SNAPSHOTS: usize = 4;

#[derive(Clone, Copy)]
struct StoredBlock {
    bytes: [u8; BLOCK_SIZE],
    crc: u32,
}

#[derive(Clone, Copy)]
struct Root {
    map: [Option<usize>; LOGICAL_BLOCKS],
    generation: u32,
}

impl Root {
    const EMPTY: Self = Self {
        map: [None; LOGICAL_BLOCKS],
        generation: 0,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageError {
    LogicalBlockOutOfRange,
    NoSpace,
    SnapshotTableFull,
    SnapshotNotFound,
    CorruptBlock,
}

/// Volume experimental volátil. Só publica uma nova raiz depois de gravar
/// todos os bytes e o checksum do bloco novo.
pub struct CowVolume {
    blocks: [Option<StoredBlock>; PHYSICAL_BLOCKS],
    current: Root,
    snapshots: [Option<Root>; SNAPSHOTS],
}

impl CowVolume {
    pub const fn new() -> Self {
        Self {
            blocks: [None; PHYSICAL_BLOCKS],
            current: Root::EMPTY,
            snapshots: [None; SNAPSHOTS],
        }
    }

    pub fn write(&mut self, logical: usize, data: &[u8; BLOCK_SIZE]) -> Result<(), StorageError> {
        if logical >= LOGICAL_BLOCKS {
            return Err(StorageError::LogicalBlockOutOfRange);
        }
        let slot = self
            .blocks
            .iter()
            .position(Option::is_none)
            .ok_or(StorageError::NoSpace)?;
        let mut next = self.current;
        self.blocks[slot] = Some(StoredBlock {
            bytes: *data,
            crc: crc32(data),
        });
        next.map[logical] = Some(slot);
        next.generation = next.generation.wrapping_add(1);
        self.current = next;
        Ok(())
    }

    pub fn read(&self, logical: usize) -> Result<[u8; BLOCK_SIZE], StorageError> {
        if logical >= LOGICAL_BLOCKS {
            return Err(StorageError::LogicalBlockOutOfRange);
        }
        let slot = match self.current.map[logical] {
            Some(slot) => slot,
            None => return Ok([0; BLOCK_SIZE]),
        };
        let block = self.blocks[slot].ok_or(StorageError::CorruptBlock)?;
        if crc32(&block.bytes) != block.crc {
            return Err(StorageError::CorruptBlock);
        }
        Ok(block.bytes)
    }

    pub fn snapshot(&mut self) -> Result<usize, StorageError> {
        let slot = self
            .snapshots
            .iter()
            .position(Option::is_none)
            .ok_or(StorageError::SnapshotTableFull)?;
        self.snapshots[slot] = Some(self.current);
        Ok(slot)
    }

    pub fn restore(&mut self, snapshot: usize) -> Result<(), StorageError> {
        let mut root = self
            .snapshots
            .get(snapshot)
            .copied()
            .flatten()
            .ok_or(StorageError::SnapshotNotFound)?;
        root.generation = self.current.generation.wrapping_add(1);
        self.current = root;
        Ok(())
    }

    /// Verifica blocos atualmente referenciados pela raiz ativa.
    pub fn scrub(&self) -> Result<usize, StorageError> {
        let mut checked = 0;
        for slot in self.current.map.iter().flatten() {
            let block = self.blocks[*slot].ok_or(StorageError::CorruptBlock)?;
            if crc32(&block.bytes) != block.crc {
                return Err(StorageError::CorruptBlock);
            }
            checked += 1;
        }
        Ok(checked)
    }

    #[cfg(test)]
    pub(crate) fn corrupt_current_block(&mut self, logical: usize) {
        if let Some(slot) = self.current.map.get(logical).copied().flatten() {
            if let Some(block) = self.blocks[slot].as_mut() {
                block.bytes[0] ^= 0xff;
            }
        }
    }
}

impl Default for CowVolume {
    fn default() -> Self {
        Self::new()
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_are_copy_on_write_and_snapshot_restores_old_root() {
        let mut fs = CowVolume::new();
        let old = [0x11; BLOCK_SIZE];
        let new = [0x22; BLOCK_SIZE];
        fs.write(2, &old).unwrap();
        let snap = fs.snapshot().unwrap();
        fs.write(2, &new).unwrap();
        assert_eq!(fs.read(2).unwrap(), new);
        fs.restore(snap).unwrap();
        assert_eq!(fs.read(2).unwrap(), old);
    }

    #[test]
    fn scrub_detects_corruption() {
        let mut fs = CowVolume::new();
        fs.write(0, &[0x5a; BLOCK_SIZE]).unwrap();
        assert_eq!(fs.scrub(), Ok(1));
        fs.corrupt_current_block(0);
        assert_eq!(fs.read(0), Err(StorageError::CorruptBlock));
        assert_eq!(fs.scrub(), Err(StorageError::CorruptBlock));
    }

    #[test]
    fn rejects_out_of_range_and_missing_snapshots() {
        let mut fs = CowVolume::new();
        assert_eq!(
            fs.write(LOGICAL_BLOCKS, &[0; BLOCK_SIZE]),
            Err(StorageError::LogicalBlockOutOfRange)
        );
        assert_eq!(fs.restore(0), Err(StorageError::SnapshotNotFound));
    }
}

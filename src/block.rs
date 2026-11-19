//! Contrato comum de bloco usado pelos filesystems do kernel.

pub const SECTOR_SIZE: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockError {
    Device,
}

/// Dispositivo lógico em setores de 512 bytes.
pub trait BlockDevice {
    fn sector_count(&self) -> u64;
    fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_SIZE]) -> Result<(), BlockError>;
    fn write_sector(&mut self, lba: u64, data: &[u8; SECTOR_SIZE]) -> Result<(), BlockError>;
    fn flush(&mut self) -> Result<(), BlockError>;
}

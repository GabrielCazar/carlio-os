//! Tabela fixa de descritores de arquivos por processo.
use crate::vfs::VFS_PATH_MAX;

/// Descritores começam após a faixa reservada às tabelas bounded de sockets.
pub const FILE_FD_BASE: u32 = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenFile {
    pub process_id: u64,
    pub descriptor: u32,
    pub path: [u8; VFS_PATH_MAX],
    pub path_len: u8,
    pub offset: u64,
    pub readable: bool,
    pub writable: bool,
    pub append: bool,
    ready: bool,
}

impl OpenFile {
    pub fn path_str(&self) -> Option<&str> {
        core::str::from_utf8(&self.path[..self.path_len as usize]).ok()
    }
}

const EMPTY: OpenFile = OpenFile {
    process_id: 0,
    descriptor: 0,
    path: [0; VFS_PATH_MAX],
    path_len: 0,
    offset: 0,
    readable: false,
    writable: false,
    append: false,
    ready: false,
};

pub struct OpenFileTable<const N: usize> {
    entries: [OpenFile; N],
}

impl<const N: usize> OpenFileTable<N> {
    pub const fn new() -> Self {
        Self {
            entries: [EMPTY; N],
        }
    }

    pub fn open(
        &mut self,
        process_id: u64,
        path: &str,
        readable: bool,
        writable: bool,
        append: bool,
    ) -> Result<u32, OpenFileError> {
        let descriptor = self.reserve(process_id, path, readable, writable, append)?;
        if let Err(error) = self.commit(process_id, descriptor) {
            let _ = self.close(process_id, descriptor);
            return Err(error);
        }
        Ok(descriptor)
    }

    /// Aloca um descritor ainda invisível para `get`; o chamador publica-o só
    /// depois de concluir a operação externa associada ao open.
    pub fn reserve(
        &mut self,
        process_id: u64,
        path: &str,
        readable: bool,
        writable: bool,
        append: bool,
    ) -> Result<u32, OpenFileError> {
        if path.is_empty() || path.len() >= VFS_PATH_MAX || !path.starts_with('/') {
            return Err(OpenFileError::InvalidPath);
        }
        let (slot, entry) = self
            .entries
            .iter_mut()
            .enumerate()
            .find(|(_, entry)| entry.process_id == 0)
            .ok_or(OpenFileError::TableFull)?;
        let descriptor = FILE_FD_BASE
            .checked_add(slot as u32)
            .ok_or(OpenFileError::TableFull)?;
        entry.path[..path.len()].copy_from_slice(path.as_bytes());
        *entry = OpenFile {
            process_id,
            descriptor,
            path: entry.path,
            path_len: path.len() as u8,
            offset: 0,
            readable,
            writable,
            append,
            ready: false,
        };
        Ok(descriptor)
    }

    pub fn commit(&mut self, process_id: u64, descriptor: u32) -> Result<(), OpenFileError> {
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| {
                entry.process_id == process_id && entry.descriptor == descriptor && !entry.ready
            })
            .ok_or(OpenFileError::BadDescriptor)?;
        entry.ready = true;
        Ok(())
    }

    pub fn get(&self, process_id: u64, descriptor: u32) -> Result<OpenFile, OpenFileError> {
        self.entries
            .iter()
            .find(|entry| {
                entry.process_id == process_id && entry.descriptor == descriptor && entry.ready
            })
            .copied()
            .ok_or(OpenFileError::BadDescriptor)
    }

    pub fn is_path_open(&self, path: &str) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.ready && entry.path_str().is_some_and(|open_path| open_path == path))
    }

    pub fn set_offset(
        &mut self,
        process_id: u64,
        descriptor: u32,
        offset: u64,
    ) -> Result<(), OpenFileError> {
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.process_id == process_id && entry.descriptor == descriptor)
            .ok_or(OpenFileError::BadDescriptor)?;
        entry.offset = offset;
        Ok(())
    }

    pub fn close(&mut self, process_id: u64, descriptor: u32) -> Result<(), OpenFileError> {
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.process_id == process_id && entry.descriptor == descriptor)
            .ok_or(OpenFileError::BadDescriptor)?;
        *entry = EMPTY;
        Ok(())
    }

    pub fn close_process(&mut self, process_id: u64) -> usize {
        if process_id == 0 {
            return 0;
        }
        let mut closed = 0usize;
        for entry in &mut self.entries {
            if entry.process_id == process_id {
                *entry = EMPTY;
                closed += 1;
            }
        }
        closed
    }
}

impl<const N: usize> Default for OpenFileTable<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenFileError {
    InvalidPath,
    TableFull,
    BadDescriptor,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptors_are_bounded_and_isolated_by_process() {
        let mut files = OpenFileTable::<2>::new();
        let first = files.open(7, "/etc/config", true, false, false).unwrap();
        assert_eq!(first, FILE_FD_BASE);
        assert_eq!(files.get(8, first), Err(OpenFileError::BadDescriptor));
        let second = files.open(8, "/var/log", false, true, true).unwrap();
        assert_eq!(second, FILE_FD_BASE + 1);
        assert_eq!(
            files.open(9, "/tmp/overflow", true, false, false),
            Err(OpenFileError::TableFull)
        );
        files.set_offset(7, first, 12).unwrap();
        assert_eq!(files.get(7, first).unwrap().offset, 12);
        files.close(7, first).unwrap();
        assert_eq!(files.get(7, first), Err(OpenFileError::BadDescriptor));
    }
}

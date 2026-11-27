//! Tabela fixa de descritores de arquivos por processo.
use crate::vfs::VFS_PATH_MAX;

/// Descritores começam após a faixa reservada às tabelas bounded de sockets.
pub const FILE_FD_BASE: u32 = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenFile {
    pub path: [u8; VFS_PATH_MAX],
    pub path_len: u8,
    pub offset: u64,
    pub readable: bool,
    pub writable: bool,
    pub append: bool,
}

impl OpenFile {
    pub fn path_str(&self) -> Option<&str> {
        core::str::from_utf8(&self.path[..self.path_len as usize]).ok()
    }
}

const EMPTY: OpenFile = OpenFile {
    path: [0; VFS_PATH_MAX],
    path_len: 0,
    offset: 0,
    readable: false,
    writable: false,
    append: false,
};

#[derive(Clone, Copy)]
struct Descriptor {
    owner: u64,
    number: u32,
    description: usize,
    close_on_exec: bool,
    ready: bool,
}

pub struct OpenFileTable<const N: usize> {
    descriptions: [OpenFile; N],
    descriptors: [Option<Descriptor>; N],
}

impl<const N: usize> OpenFileTable<N> {
    pub const fn new() -> Self {
        Self {
            descriptions: [EMPTY; N],
            descriptors: [None; N],
        }
    }

    fn descriptor_slot(&self, owner: u64, number: u32) -> Option<usize> {
        self.descriptors.iter().position(|entry| {
            entry.is_some_and(|entry| entry.owner == owner && entry.number == number)
        })
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
        let slot = self
            .descriptors
            .iter()
            .position(Option::is_none)
            .ok_or(OpenFileError::TableFull)?;
        let description = self
            .descriptions
            .iter()
            .position(|entry| entry.path_len == 0)
            .ok_or(OpenFileError::TableFull)?;
        let number = (slot..N)
            .chain(0..slot)
            .filter_map(|index| FILE_FD_BASE.checked_add(u32::try_from(index).ok()?))
            .find(|&number| self.descriptor_slot(process_id, number).is_none())
            .ok_or(OpenFileError::TableFull)?;
        let mut file = OpenFile {
            path_len: path.len() as u8,
            readable,
            writable,
            append,
            ..EMPTY
        };
        file.path[..path.len()].copy_from_slice(path.as_bytes());
        self.descriptions[description] = file;
        self.descriptors[slot] = Some(Descriptor {
            owner: process_id,
            number,
            description,
            close_on_exec: false,
            ready: false,
        });
        Ok(number)
    }

    pub fn commit(&mut self, process_id: u64, descriptor: u32) -> Result<(), OpenFileError> {
        self.commit_with_exec_flag(process_id, descriptor, false)
    }

    pub fn commit_with_exec_flag(
        &mut self,
        process_id: u64,
        descriptor: u32,
        close_on_exec: bool,
    ) -> Result<(), OpenFileError> {
        let slot = self
            .descriptor_slot(process_id, descriptor)
            .ok_or(OpenFileError::BadDescriptor)?;
        let entry = self.descriptors[slot]
            .as_mut()
            .filter(|entry| !entry.ready)
            .ok_or(OpenFileError::BadDescriptor)?;
        entry.close_on_exec = close_on_exec;
        entry.ready = true;
        Ok(())
    }

    pub fn get(&self, process_id: u64, descriptor: u32) -> Result<OpenFile, OpenFileError> {
        let slot = self
            .descriptor_slot(process_id, descriptor)
            .ok_or(OpenFileError::BadDescriptor)?;
        let entry = self.descriptors[slot]
            .filter(|entry| entry.ready)
            .ok_or(OpenFileError::BadDescriptor)?;
        Ok(self.descriptions[entry.description])
    }

    pub fn open_paths(&self) -> impl Iterator<Item = &str> {
        self.descriptors.iter().flatten().filter_map(|entry| {
            if entry.ready {
                self.descriptions[entry.description].path_str()
            } else {
                None
            }
        })
    }

    pub fn set_offset(
        &mut self,
        process_id: u64,
        descriptor: u32,
        offset: u64,
    ) -> Result<(), OpenFileError> {
        let slot = self
            .descriptor_slot(process_id, descriptor)
            .ok_or(OpenFileError::BadDescriptor)?;
        let entry = self.descriptors[slot].ok_or(OpenFileError::BadDescriptor)?;
        self.descriptions[entry.description].offset = offset;
        Ok(())
    }

    pub fn inherit(&mut self, parent: u64, child: u64) -> Result<(), OpenFileError> {
        if parent == child
            || self
                .descriptors
                .iter()
                .flatten()
                .any(|entry| entry.owner == child)
        {
            return Err(OpenFileError::BadDescriptor);
        }
        let count = self
            .descriptors
            .iter()
            .flatten()
            .filter(|entry| entry.owner == parent && entry.ready)
            .count();
        if self
            .descriptors
            .iter()
            .filter(|entry| entry.is_none())
            .count()
            < count
        {
            return Err(OpenFileError::TableFull);
        }
        for slot in 0..N {
            let Some(mut entry) =
                self.descriptors[slot].filter(|entry| entry.owner == parent && entry.ready)
            else {
                continue;
            };
            let target = self.descriptors.iter().position(Option::is_none).unwrap();
            entry.owner = child;
            self.descriptors[target] = Some(entry);
        }
        Ok(())
    }

    pub fn duplicate_to(
        &mut self,
        owner: u64,
        fd: u32,
        target: u32,
        close_on_exec: bool,
    ) -> Result<u32, OpenFileError> {
        let source = self.descriptor_slot(owner, fd).ok_or(OpenFileError::BadDescriptor)?;
        let mut alias = self.descriptors[source].filter(|entry| entry.ready)
            .ok_or(OpenFileError::BadDescriptor)?;
        if fd == target {
            return Ok(fd);
        }
        let target_slot = self.descriptor_slot(owner, target)
            .or_else(|| self.descriptors.iter().position(Option::is_none))
            .ok_or(OpenFileError::TableFull)?;
        self.close_slot(target_slot);
        alias.number = target;
        alias.close_on_exec = close_on_exec;
        self.descriptors[target_slot] = Some(alias);
        Ok(target)
    }

    fn close_slot(&mut self, slot: usize) {
        if let Some(entry) = self.descriptors[slot].take() {
            if !self
                .descriptors
                .iter()
                .flatten()
                .any(|other| other.description == entry.description)
            {
                self.descriptions[entry.description] = EMPTY;
            }
        }
    }

    pub fn close(&mut self, process_id: u64, descriptor: u32) -> Result<(), OpenFileError> {
        let slot = self
            .descriptor_slot(process_id, descriptor)
            .ok_or(OpenFileError::BadDescriptor)?;
        self.close_slot(slot);
        Ok(())
    }

    pub fn close_on_exec(&mut self, process_id: u64) {
        for slot in 0..N {
            if self.descriptors[slot]
                .is_some_and(|entry| entry.owner == process_id && entry.close_on_exec)
            {
                self.close_slot(slot);
            }
        }
    }

    pub fn close_process(&mut self, process_id: u64) -> usize {
        let mut closed = 0;
        for slot in 0..N {
            if self.descriptors[slot].is_some_and(|entry| entry.owner == process_id) {
                self.close_slot(slot);
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
    fn inheritance_shares_offset_and_keeps_close_on_exec_per_descriptor() {
        let mut files = OpenFileTable::<4>::new();
        let fd = files.reserve(7, "/keep", true, false, false).unwrap();
        files.commit_with_exec_flag(7, fd, true).unwrap();
        files.inherit(7, 8).unwrap();
        files.set_offset(8, fd, 19).unwrap();
        assert_eq!(files.get(7, fd).unwrap().offset, 19);
        files.close_on_exec(8);
        assert_eq!(files.get(8, fd), Err(OpenFileError::BadDescriptor));
        assert!(files.open_paths().any(|path| path == "/keep"));
        files.close(7, fd).unwrap();
        assert!(!files.open_paths().any(|path| path == "/keep"));
    }

    #[test]
    fn inheritance_leaves_no_descriptors_when_capacity_is_insufficient() {
        let mut files = OpenFileTable::<3>::new();
        let fd = files.open(7, "/one", true, false, false).unwrap();
        files.open(7, "/two", true, false, false).unwrap();
        assert_eq!(files.inherit(7, 8), Err(OpenFileError::TableFull));
        assert_eq!(files.get(8, fd), Err(OpenFileError::BadDescriptor));
        assert!(files.open(8, "/three", true, false, false).is_ok());
    }

    #[test]
    fn exec_closes_only_marked_descriptors_of_the_replaced_process() {
        let mut files = OpenFileTable::<3>::new();
        let inherited = files.open(7, "/keep", true, false, false).unwrap();
        let marked = files.reserve(7, "/close", true, false, false).unwrap();
        let unrelated = files.reserve(8, "/other", true, false, false).unwrap();
        files.commit_with_exec_flag(7, marked, true).unwrap();
        files.commit_with_exec_flag(8, unrelated, true).unwrap();
        files.close_on_exec(7);
        assert!(files.get(7, inherited).is_ok());
        assert_eq!(files.get(7, marked), Err(OpenFileError::BadDescriptor));
        assert!(files.get(8, unrelated).is_ok());
    }

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

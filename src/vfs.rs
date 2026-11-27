//! Núcleo inicial de VFS: árvore fixa, caminhos e permissões Unix básicas.
//!
//! VFS limitado com mounts delegáveis a backends de filesystem.

const NAME_MAX: usize = 31;
const SNAPSHOT_MAGIC: &[u8; 8] = b"CARLVFS2";
const SNAPSHOT_HEADER: usize = 20;
const MOUNT_PATH_MAX: usize = 128;
pub const VFS_PATH_MAX: usize = MOUNT_PATH_MAX;
const MOUNT_SOURCE_MAX: usize = 96;
/// Capacidade atual de arquivo por nó. O formato snapshot reserva o limite
/// total e HermFS armazena a árvore como um arquivo contíguo.
pub const VFS_INLINE_FILE_MAX: usize = 16 * 1024;

/// Backends currently understood by the namespace. The entry only describes
/// routing; a backend must still provide actual filesystem operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilesystemType {
    Hermione,
    Fat32,
    Ext4,
    Proc,
    Dev,
}

/// Interface de leitura usada pelo VFS para delegar operações em mounts.
/// Cada backend valida seus metadados e permanece dono do dispositivo de bloco.
pub trait ReadOnlyMountBackend {
    fn filesystem_type(&self) -> FilesystemType;
    fn read_file(&mut self, path: &str, output: &mut [u8]) -> Result<usize, VfsError>;

    fn metadata(&mut self, _path: &str) -> Result<MountMetadata, VfsError> {
        Err(VfsError::NotFound)
    }

    /// Reads a byte range without buffering the entire file in the kernel.
    /// Backends should override this for filesystems used by process I/O.
    fn read_file_range(
        &mut self,
        path: &str,
        offset: u64,
        output: &mut [u8],
    ) -> Result<usize, VfsError> {
        if offset == 0 {
            self.read_file(path, output)
        } else {
            Err(VfsError::NotFound)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MountMetadata {
    pub inode: u64,
    pub kind: NodeKind,
    pub uid: u32,
    pub gid: u32,
    pub mode: u16,
    pub size: u64,
}

impl MountMetadata {
    pub fn permits(self, context: AccessContext<'_>, requested: Access) -> bool {
        if context.credentials.uid == 0 {
            return !requested.contains(Access::EXECUTE)
                || self.kind == NodeKind::Directory
                || self.mode & 0o111 != 0;
        }
        let shift = if context.credentials.uid == self.uid {
            6
        } else if context.in_group(self.gid) {
            3
        } else {
            0
        };
        Access(((self.mode >> shift) & 7) as u8).contains(requested)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MountOptions {
    pub read_only: bool,
    pub no_exec: bool,
    pub no_suid: bool,
}

impl MountOptions {
    pub const DEFAULT: Self = Self {
        read_only: false,
        no_exec: false,
        no_suid: false,
    };
}

#[derive(Clone, Copy)]
struct MountRecord {
    path: [u8; MOUNT_PATH_MAX],
    path_len: u8,
    source: [u8; MOUNT_SOURCE_MAX],
    source_len: u8,
    filesystem: FilesystemType,
    options: MountOptions,
    occupied: bool,
}

impl MountRecord {
    const EMPTY: Self = Self {
        path: [0; MOUNT_PATH_MAX],
        path_len: 0,
        source: [0; MOUNT_SOURCE_MAX],
        source_len: 0,
        filesystem: FilesystemType::Hermione,
        options: MountOptions::DEFAULT,
        occupied: false,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MountInfo<'a> {
    pub path: &'a str,
    pub source: &'a str,
    pub filesystem: FilesystemType,
    pub options: MountOptions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Directory,
    File,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Credentials {
    pub uid: u32,
    pub gid: u32,
}

/// Credenciais POSIX da operação. Grupos suplementares participam da seleção
/// owner/group/other, como em Linux; `umask` é aplicado ao criar nós.
#[derive(Clone, Copy, Debug)]
pub struct AccessContext<'a> {
    pub credentials: Credentials,
    pub supplementary_groups: &'a [u32],
    pub umask: u16,
}

impl<'a> AccessContext<'a> {
    pub const fn new(
        credentials: Credentials,
        supplementary_groups: &'a [u32],
        umask: u16,
    ) -> Self {
        Self {
            credentials,
            supplementary_groups,
            umask: umask & 0o777,
        }
    }

    fn in_group(self, gid: u32) -> bool {
        self.credentials.gid == gid || self.supplementary_groups.contains(&gid)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Access(u8);

impl Access {
    pub const READ: Self = Self(4);
    pub const WRITE: Self = Self(2);
    pub const EXECUTE: Self = Self(1);
    pub const READ_WRITE: Self = Self(6);
    pub const READ_EXECUTE: Self = Self(5);

    pub const fn from_posix_bits(bits: u8) -> Option<Self> {
        if bits & !0b111 == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    const fn contains(self, requested: Self) -> bool {
        self.0 & requested.0 == requested.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VfsError {
    InvalidPath,
    NotFound,
    AlreadyExists,
    NotDirectory,
    PermissionDenied,
    Full,
    NotOwner,
    NotExecutable,
    IsDirectory,
    DirectoryNotEmpty,
    Busy,
    BufferTooSmall,
    FileTooLarge,
    MountTableFull,
    AlreadyMounted,
    NotMounted,
    MountDenied,
    InvalidMountPath,
}

#[derive(Clone, Copy)]
struct Node {
    name: [u8; NAME_MAX],
    parent: usize,
    kind: NodeKind,
    uid: u32,
    gid: u32,
    mode: u16,
    occupied: bool,
}

impl Node {
    const EMPTY: Self = Self {
        name: [0; NAME_MAX],
        parent: 0,
        kind: NodeKind::File,
        uid: 0,
        gid: 0,
        mode: 0,
        occupied: false,
    };
}

/// Árvore VFS limitada, adequada à fase anterior ao heap.
pub struct Vfs<const N: usize> {
    nodes: [Node; N],
    contents: [[u8; VFS_INLINE_FILE_MAX]; N],
    content_lengths: [u16; N],
    mounts: [MountRecord; N],
    len: usize,
}

impl<const N: usize> Vfs<N> {
    pub const fn new() -> Self {
        let mut nodes = [Node::EMPTY; N];
        if N > 0 {
            nodes[0] = Node {
                name: [0; NAME_MAX],
                parent: 0,
                kind: NodeKind::Directory,
                uid: 0,
                gid: 0,
                mode: 0o755,
                occupied: true,
            };
        }
        Self {
            nodes,
            contents: [[0; VFS_INLINE_FILE_MAX]; N],
            content_lengths: [0; N],
            mounts: [MountRecord::EMPTY; N],
            len: if N > 0 { 1 } else { 0 },
        }
    }

    pub const SNAPSHOT_MAX: usize = SNAPSHOT_HEADER + N * (18 + NAME_MAX + VFS_INLINE_FILE_MAX);

    pub fn restore_overlay(&mut self, input: &[u8]) -> Result<(), VfsError> {
        let mounts = self.mounts;
        let result = self.restore_snapshot(input);
        self.mounts = mounts;
        result
    }

    /// Serializa a árvore VFS e o conteúdo dos arquivos para persistência.
    /// Mounts são estado de runtime e não fazem parte do snapshot.
    pub fn encode_snapshot(&self, output: &mut [u8]) -> Result<usize, VfsError> {
        if output.len() < SNAPSHOT_HEADER {
            return Err(VfsError::BufferTooSmall);
        }
        output[..SNAPSHOT_HEADER].fill(0);
        output[..8].copy_from_slice(SNAPSHOT_MAGIC);
        output[8..10].copy_from_slice(&2u16.to_le_bytes());
        let occupied = self.nodes[..self.len]
            .iter()
            .filter(|node| node.occupied)
            .count();
        output[10..12].copy_from_slice(&(occupied as u16).to_le_bytes());
        let mut cursor = SNAPSHOT_HEADER;
        for index in 0..self.len {
            let node = self.nodes[index];
            if !node.occupied {
                continue;
            }
            let name_len = node
                .name
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(NAME_MAX);
            let data_len = if node.kind == NodeKind::File {
                self.content_lengths[index] as usize
            } else {
                0
            };
            let record_len = 18usize
                .checked_add(name_len)
                .and_then(|n| n.checked_add(data_len))
                .ok_or(VfsError::FileTooLarge)?;
            let end = cursor
                .checked_add(record_len)
                .ok_or(VfsError::FileTooLarge)?;
            if end > output.len() {
                return Err(VfsError::BufferTooSmall);
            }
            output[cursor..cursor + 2].copy_from_slice(&(index as u16).to_le_bytes());
            output[cursor + 2..cursor + 4].copy_from_slice(&(node.parent as u16).to_le_bytes());
            output[cursor + 4] = if node.kind == NodeKind::Directory {
                1
            } else {
                0
            };
            output[cursor + 5..cursor + 9].copy_from_slice(&node.uid.to_le_bytes());
            output[cursor + 9..cursor + 13].copy_from_slice(&node.gid.to_le_bytes());
            output[cursor + 13..cursor + 15].copy_from_slice(&node.mode.to_le_bytes());
            output[cursor + 15..cursor + 17].copy_from_slice(&(data_len as u16).to_le_bytes());
            output[cursor + 17] = name_len as u8;
            output[cursor + 18..cursor + 18 + name_len].copy_from_slice(&node.name[..name_len]);
            output[cursor + 18 + name_len..end].copy_from_slice(&self.contents[index][..data_len]);
            cursor = end;
        }
        output[12..16].copy_from_slice(&((cursor - SNAPSHOT_HEADER) as u32).to_le_bytes());
        let checksum = snapshot_crc(&output[..cursor]);
        output[16..20].copy_from_slice(&checksum.to_le_bytes());
        Ok(cursor)
    }

    /// Restaura um snapshot validado sem aceitar índices duplicados, pais
    /// inválidos, nomes malformados ou conteúdo além dos limites do VFS.
    pub fn decode_snapshot(input: &[u8]) -> Result<Self, VfsError> {
        let mut vfs = Self::new();
        vfs.restore_snapshot(input)?;
        Ok(vfs)
    }

    /// Variante in-place usada no kernel para evitar colocar uma VFS inteira
    /// (incluindo buffers) na stack de boot.
    pub fn restore_snapshot(&mut self, input: &[u8]) -> Result<(), VfsError> {
        let is_v1 = input.len() >= SNAPSHOT_HEADER
            && &input[..8] == b"CARLVFS1"
            && u16::from_le_bytes([input[8], input[9]]) == 1;
        let is_v2 = input.len() >= SNAPSHOT_HEADER
            && &input[..8] == SNAPSHOT_MAGIC
            && u16::from_le_bytes([input[8], input[9]]) == 2;
        if !is_v1 && !is_v2 {
            return Err(VfsError::InvalidPath);
        }
        let count = u16::from_le_bytes([input[10], input[11]]) as usize;
        let payload = u32::from_le_bytes(
            input[12..16]
                .try_into()
                .map_err(|_| VfsError::InvalidPath)?,
        ) as usize;
        let expected = u32::from_le_bytes(
            input[16..20]
                .try_into()
                .map_err(|_| VfsError::InvalidPath)?,
        );
        if count == 0
            || count > N
            || payload != input.len() - SNAPSHOT_HEADER
            || snapshot_crc(input) != expected
        {
            return Err(VfsError::InvalidPath);
        }
        self.nodes.fill(Node::EMPTY);
        self.contents.fill([0; VFS_INLINE_FILE_MAX]);
        self.content_lengths.fill(0);
        self.mounts.fill(MountRecord::EMPTY);
        self.len = 0;
        let mut seen = [false; N];
        let mut cursor = SNAPSHOT_HEADER;
        for _ in 0..count {
            if input.len().saturating_sub(cursor) < 18 {
                return Err(VfsError::InvalidPath);
            }
            let index = u16::from_le_bytes([input[cursor], input[cursor + 1]]) as usize;
            let parent = u16::from_le_bytes([input[cursor + 2], input[cursor + 3]]) as usize;
            let kind = match input[cursor + 4] {
                0 => NodeKind::File,
                1 => NodeKind::Directory,
                _ => return Err(VfsError::InvalidPath),
            };
            let uid = u32::from_le_bytes(
                input[cursor + 5..cursor + 9]
                    .try_into()
                    .map_err(|_| VfsError::InvalidPath)?,
            );
            let gid = u32::from_le_bytes(
                input[cursor + 9..cursor + 13]
                    .try_into()
                    .map_err(|_| VfsError::InvalidPath)?,
            );
            let mode = u16::from_le_bytes([input[cursor + 13], input[cursor + 14]]);
            let data_len = u16::from_le_bytes([input[cursor + 15], input[cursor + 16]]) as usize;
            let name_len = input[cursor + 17] as usize;
            let end = cursor
                .checked_add(18)
                .and_then(|n| n.checked_add(name_len))
                .and_then(|n| n.checked_add(data_len))
                .ok_or(VfsError::InvalidPath)?;
            if index >= N
                || seen[index]
                || name_len > NAME_MAX
                || data_len > VFS_INLINE_FILE_MAX
                || end > input.len()
                || (kind == NodeKind::Directory && data_len != 0)
            {
                return Err(VfsError::InvalidPath);
            }
            let name_start = cursor + 18;
            let name = &input[name_start..name_start + name_len];
            if index == 0 {
                if parent != 0 || kind != NodeKind::Directory || name_len != 0 {
                    return Err(VfsError::InvalidPath);
                }
            } else if parent >= N
                || !seen[parent]
                || self.nodes[parent].kind != NodeKind::Directory
                || name.is_empty()
                || !name
                    .iter()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(byte))
            {
                return Err(VfsError::InvalidPath);
            }
            let mut node_name = [0; NAME_MAX];
            node_name[..name_len].copy_from_slice(name);
            self.nodes[index] = Node {
                name: node_name,
                parent,
                kind,
                uid,
                gid,
                mode,
                occupied: true,
            };
            if data_len != 0 {
                self.contents[index][..data_len]
                    .copy_from_slice(&input[name_start + name_len..end]);
            }
            self.content_lengths[index] = data_len as u16;
            seen[index] = true;
            self.len = self.len.max(index + 1);
            cursor = end;
        }
        if cursor != input.len() || !seen[0] {
            return Err(VfsError::InvalidPath);
        }
        for left in 1..N {
            if !seen[left] {
                continue;
            }
            for right in left + 1..N {
                if !seen[right] || self.nodes[left].parent != self.nodes[right].parent {
                    continue;
                }
                let left_name = &self.nodes[left].name[..self.nodes[left]
                    .name
                    .iter()
                    .position(|b| *b == 0)
                    .unwrap_or(NAME_MAX)];
                let right_name = &self.nodes[right].name[..self.nodes[right]
                    .name
                    .iter()
                    .position(|b| *b == 0)
                    .unwrap_or(NAME_MAX)];
                if left_name == right_name {
                    return Err(VfsError::AlreadyExists);
                }
            }
        }
        Ok(())
    }

    /// Monta diretórios usuais do sistema; conteúdo e serviços vêm depois.
    pub fn create_system_layout(&mut self) -> Result<(), VfsError> {
        let root = Credentials { uid: 0, gid: 0 };
        for name in [
            "programas",
            "configuracoes",
            "usuarios",
            "sistema",
            "dados",
            "temporarios",
        ] {
            self.create_dir("/", name, root, 0o755)?;
        }
        self.create_dir("/usuarios", "convidado", root, 0o755)?;
        self.create_dir("/configuracoes", "servicos", root, 0o750)?;
        Ok(())
    }

    /// Cria a hierarquia de caminhos FHS usada por userlands Arch e Linux.
    /// Entradas `bin`/`lib` ainda são diretórios: symlinks e resolução de
    /// aliases permanecem fora do VFS atual.
    pub fn create_fhs_layout(&mut self) -> Result<(), VfsError> {
        let root = Credentials { uid: 0, gid: 0 };
        for name in [
            "bin", "boot", "dev", "etc", "home", "lib", "lib64", "media", "mnt", "opt", "proc",
            "root", "run", "sbin", "srv", "sys", "tmp", "usr", "var",
        ] {
            let mode = match name {
                "root" => 0o700,
                "tmp" => 0o1777,
                _ => 0o755,
            };
            self.create_dir("/", name, root, mode)?;
        }
        for (parent, child, mode) in [
            ("/usr", "bin", 0o755),
            ("/usr", "lib", 0o755),
            ("/usr", "sbin", 0o755),
            ("/usr", "share", 0o755),
            ("/var", "cache", 0o755),
            ("/var", "lib", 0o755),
            ("/var", "log", 0o755),
            ("/var", "tmp", 0o1777),
        ] {
            self.create_dir(parent, child, root, mode)?;
        }
        Ok(())
    }

    /// Registra um backend em um diretório existente. Neste estágio, mount
    /// determina o roteamento do namespace; operações de I/O seguem limitadas
    /// aos backends que já foram conectados ao kernel.
    pub fn mount(
        &mut self,
        source: &str,
        target: &str,
        filesystem: FilesystemType,
        options: MountOptions,
        credentials: Credentials,
    ) -> Result<(), VfsError> {
        if credentials.uid != 0 {
            return Err(VfsError::MountDenied);
        }
        if !valid_mount_path(target)
            || source.is_empty()
            || source.len() >= MOUNT_SOURCE_MAX
            || source.bytes().any(|b| b.is_ascii_control())
        {
            return Err(VfsError::InvalidMountPath);
        }
        let target_index = self.resolve(target, AccessContext::new(credentials, &[], 0))?;
        if self.nodes[target_index].kind != NodeKind::Directory {
            return Err(VfsError::NotDirectory);
        }
        if self.mounts.iter().any(|entry| {
            entry.occupied
                && entry.path_len as usize == target.len()
                && &entry.path[..target.len()] == target.as_bytes()
        }) {
            return Err(VfsError::AlreadyMounted);
        }
        let slot = self
            .mounts
            .iter_mut()
            .find(|entry| !entry.occupied)
            .ok_or(VfsError::MountTableFull)?;
        slot.path[..target.len()].copy_from_slice(target.as_bytes());
        slot.path_len = target.len() as u8;
        slot.source[..source.len()].copy_from_slice(source.as_bytes());
        slot.source_len = source.len() as u8;
        slot.filesystem = filesystem;
        slot.options = options;
        slot.occupied = true;
        Ok(())
    }

    pub fn unmount(&mut self, target: &str, credentials: Credentials) -> Result<(), VfsError> {
        if credentials.uid != 0 {
            return Err(VfsError::MountDenied);
        }
        let entry = self
            .mounts
            .iter_mut()
            .find(|entry| {
                entry.occupied
                    && entry.path_len as usize == target.len()
                    && &entry.path[..target.len()] == target.as_bytes()
            })
            .ok_or(VfsError::NotMounted)?;
        *entry = MountRecord::EMPTY;
        Ok(())
    }

    /// Retorna o mount mais específico que cobre um caminho absoluto.
    pub fn mount_for(&self, path: &str) -> Option<MountInfo<'_>> {
        if !valid_mount_path(path) {
            return None;
        }
        self.mounts
            .iter()
            .filter(|entry| {
                entry.occupied && path_matches_mount(path, &entry.path[..entry.path_len as usize])
            })
            .max_by_key(|entry| entry.path_len)
            .and_then(|entry| {
                Some(MountInfo {
                    path: core::str::from_utf8(&entry.path[..entry.path_len as usize]).ok()?,
                    source: core::str::from_utf8(&entry.source[..entry.source_len as usize])
                        .ok()?,
                    filesystem: entry.filesystem,
                    options: entry.options,
                })
            })
    }

    /// Lê um arquivo por meio do backend do mount mais específico.
    /// O chamador passa o backend associado ao dispositivo de `source`.
    pub fn read_mounted_file<B: ReadOnlyMountBackend>(
        &self,
        path: &str,
        context: AccessContext<'_>,
        backend: &mut B,
        output: &mut [u8],
    ) -> Result<usize, VfsError> {
        let relative = self.mounted_path(path, context, backend)?;
        let metadata = backend.metadata(relative)?;
        if !metadata.permits(context, Access::READ) {
            return Err(VfsError::PermissionDenied);
        }
        backend.read_file(relative, output)
    }

    /// Reads a byte range from the backend selected by the most specific mount.
    /// The backend remains responsible for filesystem metadata validation.
    pub fn read_mounted_file_range<B: ReadOnlyMountBackend>(
        &self,
        path: &str,
        context: AccessContext<'_>,
        backend: &mut B,
        offset: u64,
        output: &mut [u8],
    ) -> Result<usize, VfsError> {
        let relative = self.mounted_path(path, context, backend)?;
        let metadata = backend.metadata(relative)?;
        if !metadata.permits(context, Access::READ) {
            return Err(VfsError::PermissionDenied);
        }
        backend.read_file_range(relative, offset, output)
    }

    pub fn mounted_metadata<B: ReadOnlyMountBackend>(
        &self,
        path: &str,
        context: AccessContext<'_>,
        backend: &mut B,
    ) -> Result<MountMetadata, VfsError> {
        let relative = self.mounted_path(path, context, backend)?;
        backend.metadata(relative)
    }

    /// Preserve the mounted directory's authorization data when projecting
    /// it into the writable namespace.
    pub fn materialize_mounted_directory<B: ReadOnlyMountBackend>(
        &mut self,
        path: &str,
        context: AccessContext<'_>,
        backend: &mut B,
    ) -> Result<(), VfsError> {
        let metadata = self.mounted_metadata(path, context, backend)?;
        if metadata.kind != NodeKind::Directory {
            return Err(VfsError::NotDirectory);
        }
        if !metadata.permits(context, Access::EXECUTE) {
            return Err(VfsError::PermissionDenied);
        }
        let (parent, name) = path.rsplit_once('/').ok_or(VfsError::InvalidPath)?;
        let parent = if parent.is_empty() { "/" } else { parent };
        let root = AccessContext::new(Credentials { uid: 0, gid: 0 }, &[], 0);
        self.create_dir_as(parent, name, root, metadata.mode)?;
        let index = self.resolve(path, root)?;
        self.nodes[index].uid = metadata.uid;
        self.nodes[index].gid = metadata.gid;
        self.nodes[index].mode = metadata.mode & 0o7777;
        Ok(())
    }

    fn mounted_path<'a, B: ReadOnlyMountBackend>(
        &self,
        path: &'a str,
        context: AccessContext<'_>,
        backend: &mut B,
    ) -> Result<&'a str, VfsError> {
        let mount = self.mount_for(path).ok_or(VfsError::NotMounted)?;
        if mount.filesystem != backend.filesystem_type() {
            return Err(VfsError::InvalidMountPath);
        }
        if !mount.options.read_only {
            return Err(VfsError::PermissionDenied);
        }
        let mount_node = self.resolve(mount.path, context)?;
        self.check_node_access(mount_node, context, Access::EXECUTE)?;
        let relative = if path == mount.path {
            "/"
        } else if mount.path == "/" {
            path
        } else {
            &path[mount.path.len()..]
        };
        if relative != "/" {
            let directories = core::iter::once("/").chain(
                relative
                    .match_indices('/')
                    .skip(1)
                    .map(|(end, _)| &relative[..end]),
            );
            for directory in directories {
                let metadata = backend.metadata(directory)?;
                if metadata.kind != NodeKind::Directory {
                    return Err(VfsError::NotDirectory);
                }
                if !metadata.permits(context, Access::EXECUTE) {
                    return Err(VfsError::PermissionDenied);
                }
            }
        }
        Ok(relative)
    }

    /// Valida execução em arquivo fornecido por um mount. Permissões vêm do
    /// inode do backend; a opção `no_exec` do mount sempre prevalece.
    pub fn authorize_mounted_execution<B: ReadOnlyMountBackend>(
        &self,
        path: &str,
        context: AccessContext<'_>,
        backend: &mut B,
    ) -> Result<(), VfsError> {
        let mount = self.mount_for(path).ok_or(VfsError::NotMounted)?;
        if mount.options.no_exec {
            return Err(VfsError::NotExecutable);
        }
        let metadata = self.mounted_metadata(path, context, backend)?;
        if metadata.kind != NodeKind::File || !metadata.permits(context, Access::READ_EXECUTE) {
            return Err(VfsError::NotExecutable);
        }
        Ok(())
    }

    pub fn create_dir(
        &mut self,
        parent_path: &str,
        name: &str,
        owner: Credentials,
        mode: u16,
    ) -> Result<(), VfsError> {
        self.create_node(parent_path, name, owner, mode, NodeKind::Directory)
    }

    pub fn create_file(
        &mut self,
        parent_path: &str,
        name: &str,
        owner: Credentials,
        mode: u16,
    ) -> Result<(), VfsError> {
        self.create_node(parent_path, name, owner, mode, NodeKind::File)
    }

    pub fn check_access(
        &self,
        path: &str,
        credentials: Credentials,
        requested: Access,
    ) -> Result<NodeKind, VfsError> {
        self.check_access_as(path, AccessContext::new(credentials, &[], 0), requested)
    }

    pub fn check_access_as(
        &self,
        path: &str,
        context: AccessContext<'_>,
        requested: Access,
    ) -> Result<NodeKind, VfsError> {
        let index = self.resolve(path, context)?;
        let node = self.nodes[index];
        self.check_node_access(index, context, requested)?;
        Ok(node.kind)
    }

    /// Marca a intenção executável e exige permissão de execução no acesso.
    /// O carregador ELF/processo será conectado quando houver ring 3.
    pub fn authorize_execution(
        &self,
        path: &str,
        credentials: Credentials,
    ) -> Result<(), VfsError> {
        self.authorize_execution_as(path, AccessContext::new(credentials, &[], 0))
    }

    pub fn authorize_execution_as(
        &self,
        path: &str,
        context: AccessContext<'_>,
    ) -> Result<(), VfsError> {
        let kind = self.check_access_as(path, context, Access::EXECUTE)?;
        if kind != NodeKind::File {
            return Err(VfsError::NotExecutable);
        }
        Ok(())
    }

    /// Grava conteúdo inline para os arquivos pequenos de configuração e
    /// bootstrap. A capacidade por arquivo é fixa até a integração do heap/VFS.
    pub fn write_file_as(
        &mut self,
        path: &str,
        context: AccessContext<'_>,
        bytes: &[u8],
    ) -> Result<(), VfsError> {
        if bytes.len() > VFS_INLINE_FILE_MAX {
            return Err(VfsError::FileTooLarge);
        }
        let index = self.resolve(path, context)?;
        if self.nodes[index].kind != NodeKind::File {
            return Err(VfsError::IsDirectory);
        }
        self.check_node_access(index, context, Access::WRITE)?;
        self.contents[index].fill(0);
        self.contents[index][..bytes.len()].copy_from_slice(bytes);
        self.content_lengths[index] = bytes.len() as u16;
        Ok(())
    }

    pub fn read_file_range_as(
        &self,
        path: &str,
        context: AccessContext<'_>,
        offset: usize,
        output: &mut [u8],
    ) -> Result<usize, VfsError> {
        let index = self.resolve(path, context)?;
        if self.nodes[index].kind != NodeKind::File {
            return Err(VfsError::IsDirectory);
        }
        self.check_node_access(index, context, Access::READ)?;
        let length = self.content_lengths[index] as usize;
        if offset >= length || output.is_empty() {
            return Ok(0);
        }
        let amount = output.len().min(length - offset);
        output[..amount].copy_from_slice(&self.contents[index][offset..offset + amount]);
        Ok(amount)
    }

    pub fn write_file_range_as(
        &mut self,
        path: &str,
        context: AccessContext<'_>,
        offset: usize,
        bytes: &[u8],
    ) -> Result<usize, VfsError> {
        let end = offset
            .checked_add(bytes.len())
            .ok_or(VfsError::FileTooLarge)?;
        if end > VFS_INLINE_FILE_MAX {
            return Err(VfsError::FileTooLarge);
        }
        let index = self.resolve(path, context)?;
        if self.nodes[index].kind != NodeKind::File {
            return Err(VfsError::IsDirectory);
        }
        self.check_node_access(index, context, Access::WRITE)?;
        if offset > self.content_lengths[index] as usize {
            self.contents[index][self.content_lengths[index] as usize..offset].fill(0);
        }
        self.contents[index][offset..end].copy_from_slice(bytes);
        self.content_lengths[index] = (self.content_lengths[index] as usize).max(end) as u16;
        Ok(bytes.len())
    }

    pub fn truncate_file_as(
        &mut self,
        path: &str,
        context: AccessContext<'_>,
    ) -> Result<(), VfsError> {
        let index = self.resolve(path, context)?;
        if self.nodes[index].kind != NodeKind::File {
            return Err(VfsError::IsDirectory);
        }
        self.check_node_access(index, context, Access::WRITE)?;
        self.contents[index].fill(0);
        self.content_lengths[index] = 0;
        Ok(())
    }

    pub fn file_len_as(
        &self,
        path: &str,
        context: AccessContext<'_>,
        requested: Access,
    ) -> Result<usize, VfsError> {
        let index = self.resolve(path, context)?;
        if self.nodes[index].kind != NodeKind::File {
            return Err(VfsError::IsDirectory);
        }
        self.check_node_access(index, context, requested)?;
        Ok(self.content_lengths[index] as usize)
    }

    /// Lê um arquivo após validar permissão de busca nos pais e leitura no nó.
    pub fn read_file_as(
        &self,
        path: &str,
        context: AccessContext<'_>,
        output: &mut [u8],
    ) -> Result<usize, VfsError> {
        let index = self.resolve(path, context)?;
        if self.nodes[index].kind != NodeKind::File {
            return Err(VfsError::IsDirectory);
        }
        self.check_node_access(index, context, Access::READ)?;
        let length = self.content_lengths[index] as usize;
        if output.len() < length {
            return Err(VfsError::BufferTooSmall);
        }
        output[..length].copy_from_slice(&self.contents[index][..length]);
        Ok(length)
    }

    /// Remove um arquivo pela permissão de escrita/busca do diretório pai.
    /// Diretórios não vazios e remoção através de diretório sticky sem
    /// privilégio são recusados.
    pub fn remove_file_as(
        &mut self,
        path: &str,
        context: AccessContext<'_>,
    ) -> Result<(), VfsError> {
        self.remove_file_unless_open_as(path, context, core::iter::empty())
    }

    pub fn remove_file_unless_open_as<'a>(
        &mut self,
        path: &str,
        context: AccessContext<'_>,
        open_paths: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), VfsError> {
        let (parent_path, name) = path.rsplit_once('/').ok_or(VfsError::InvalidPath)?;
        if name.is_empty()
            || name.len() >= NAME_MAX
            || name == "."
            || name == ".."
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            return Err(VfsError::InvalidPath);
        }
        let parent_path = if parent_path.is_empty() {
            "/"
        } else {
            parent_path
        };
        let parent = self.resolve(parent_path, context)?;
        if self.nodes[parent].kind != NodeKind::Directory {
            return Err(VfsError::NotDirectory);
        }
        self.check_node_access(parent, context, Access::WRITE_EXECUTE)?;
        let name_bytes = name.as_bytes();
        let index = self
            .find_child(parent, name_bytes)
            .ok_or(VfsError::NotFound)?;
        if self.nodes[index].kind == NodeKind::Directory {
            return Err(VfsError::IsDirectory);
        }
        let credentials = context.credentials;
        if self.nodes[parent].mode & 0o1000 != 0
            && credentials.uid != 0
            && credentials.uid != self.nodes[parent].uid
            && credentials.uid != self.nodes[index].uid
        {
            return Err(VfsError::NotOwner);
        }
        // Authorization above uses the caller. Comparing existing descriptors
        // uses the namespace independently of their owners' current access.
        let root = AccessContext::new(Credentials { uid: 0, gid: 0 }, &[], 0);
        if open_paths
            .into_iter()
            .any(|open_path| self.resolve(open_path, root) == Ok(index))
        {
            return Err(VfsError::Busy);
        }
        self.nodes[index].occupied = false;
        self.nodes[index].name.fill(0);
        self.contents[index].fill(0);
        self.content_lengths[index] = 0;
        Ok(())
    }

    /// Remove um diretório vazio com as mesmas regras de DAC e sticky bit do
    /// unlink. Um ponto de mount permanece preso ao namespace.
    pub fn remove_dir_as(
        &mut self,
        path: &str,
        context: AccessContext<'_>,
    ) -> Result<(), VfsError> {
        let (parent_path, name) = path.rsplit_once('/').ok_or(VfsError::InvalidPath)?;
        if name.is_empty()
            || name.len() >= NAME_MAX
            || name == "."
            || name == ".."
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            return Err(VfsError::InvalidPath);
        }
        let parent_path = if parent_path.is_empty() {
            "/"
        } else {
            parent_path
        };
        let parent = self.resolve(parent_path, context)?;
        if self.nodes[parent].kind != NodeKind::Directory {
            return Err(VfsError::NotDirectory);
        }
        self.check_node_access(parent, context, Access::WRITE_EXECUTE)?;
        let index = self
            .find_child(parent, name.as_bytes())
            .ok_or(VfsError::NotFound)?;
        if self.nodes[index].kind != NodeKind::Directory {
            return Err(VfsError::NotDirectory);
        }
        if self.nodes[..self.len]
            .iter()
            .any(|node| node.occupied && node.parent == index)
        {
            return Err(VfsError::DirectoryNotEmpty);
        }
        for mount in &self.mounts {
            if !mount.occupied {
                continue;
            }
            let mount_path = core::str::from_utf8(&mount.path[..mount.path_len as usize])
                .map_err(|_| VfsError::InvalidPath)?;
            let root_context = AccessContext::new(Credentials { uid: 0, gid: 0 }, &[], 0);
            let mut mount_node = self.resolve(mount_path, root_context)?;
            loop {
                if mount_node == index {
                    return Err(VfsError::Busy);
                }
                let ancestor = self.nodes[mount_node].parent;
                if ancestor == mount_node {
                    break;
                }
                mount_node = ancestor;
            }
        }
        let credentials = context.credentials;
        if self.nodes[parent].mode & 0o1000 != 0
            && credentials.uid != 0
            && credentials.uid != self.nodes[parent].uid
            && credentials.uid != self.nodes[index].uid
        {
            return Err(VfsError::NotOwner);
        }
        self.nodes[index].occupied = false;
        self.nodes[index].name.fill(0);
        self.contents[index].fill(0);
        self.content_lengths[index] = 0;
        Ok(())
    }

    pub fn set_mode(
        &mut self,
        path: &str,
        credentials: Credentials,
        mode: u16,
    ) -> Result<(), VfsError> {
        self.set_mode_as(path, AccessContext::new(credentials, &[], 0), mode)
    }

    pub fn set_mode_as(
        &mut self,
        path: &str,
        context: AccessContext<'_>,
        mode: u16,
    ) -> Result<(), VfsError> {
        let index = self.resolve(path, context)?;
        let credentials = context.credentials;
        if credentials.uid != 0 && credentials.uid != self.nodes[index].uid {
            return Err(VfsError::NotOwner);
        }
        self.nodes[index].mode = mode & 0o7777;
        Ok(())
    }

    /// Altera proprietário/grupo somente como root; demais processos recebem
    /// `NotOwner`. Limita os bits aos modos POSIX suportados pelo VFS.
    pub fn set_owner(
        &mut self,
        path: &str,
        credentials: Credentials,
        uid: u32,
        gid: u32,
    ) -> Result<(), VfsError> {
        self.set_owner_as(path, AccessContext::new(credentials, &[], 0), uid, gid)
    }

    pub fn set_owner_as(
        &mut self,
        path: &str,
        context: AccessContext<'_>,
        uid: u32,
        gid: u32,
    ) -> Result<(), VfsError> {
        if context.credentials.uid != 0 {
            return Err(VfsError::NotOwner);
        }
        let index = self.resolve(path, context)?;
        let owner_changed = self.nodes[index].uid != uid || self.nodes[index].gid != gid;
        self.nodes[index].uid = uid;
        self.nodes[index].gid = gid;
        if owner_changed {
            self.nodes[index].mode &= !0o6000;
        }
        Ok(())
    }

    pub fn metadata(&self, path: &str, context: AccessContext<'_>) -> Result<Metadata, VfsError> {
        let index = self.resolve(path, context)?;
        let node = self.nodes[index];
        Ok(Metadata {
            inode: u64::try_from(index)
                .ok()
                .and_then(|value| value.checked_add(1))
                .ok_or(VfsError::Full)?,
            kind: node.kind,
            uid: node.uid,
            gid: node.gid,
            mode: node.mode,
            size: u64::from(self.content_lengths[index]),
        })
    }

    fn create_node(
        &mut self,
        parent_path: &str,
        name: &str,
        owner: Credentials,
        mode: u16,
        kind: NodeKind,
    ) -> Result<(), VfsError> {
        self.create_node_as(
            parent_path,
            name,
            AccessContext::new(owner, &[], 0),
            owner,
            mode,
            kind,
        )
    }

    pub fn create_file_as(
        &mut self,
        parent_path: &str,
        name: &str,
        context: AccessContext<'_>,
        mode: u16,
    ) -> Result<(), VfsError> {
        self.create_node_as(
            parent_path,
            name,
            context,
            context.credentials,
            mode,
            NodeKind::File,
        )
    }

    pub fn create_dir_as(
        &mut self,
        parent_path: &str,
        name: &str,
        context: AccessContext<'_>,
        mode: u16,
    ) -> Result<(), VfsError> {
        self.create_node_as(
            parent_path,
            name,
            context,
            context.credentials,
            mode,
            NodeKind::Directory,
        )
    }

    fn create_node_as(
        &mut self,
        parent_path: &str,
        name: &str,
        context: AccessContext<'_>,
        owner: Credentials,
        mode: u16,
        kind: NodeKind,
    ) -> Result<(), VfsError> {
        let name_bytes = name.as_bytes();
        if name_bytes.is_empty()
            || name_bytes.len() >= NAME_MAX
            || name == "."
            || name == ".."
            || !name_bytes
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(b))
        {
            return Err(VfsError::InvalidPath);
        }
        let parent = self.resolve(parent_path, context)?;
        if self.nodes[parent].kind != NodeKind::Directory {
            return Err(VfsError::NotDirectory);
        }
        self.check_access_as(parent_path, context, Access::WRITE_EXECUTE)?;
        if self.find_child(parent, name_bytes).is_some() {
            return Err(VfsError::AlreadyExists);
        }
        let slot = (0..self.len)
            .find(|index| !self.nodes[*index].occupied)
            .or_else(|| (self.len < N).then_some(self.len))
            .ok_or(VfsError::Full)?;
        let mut encoded = [0; NAME_MAX];
        encoded[..name_bytes.len()].copy_from_slice(name_bytes);
        let parent_gid = self.nodes[parent].gid;
        let inherit_setgid = self.nodes[parent].mode & 0o2000 != 0;
        let mut child_mode = (mode & 0o7777) & !(context.umask & 0o777);
        if inherit_setgid && kind == NodeKind::Directory {
            child_mode |= 0o2000;
        }
        if context.credentials.uid != 0
            && (child_mode & 0o4000 != 0
                || (child_mode & 0o2000 != 0 && !context.in_group(parent_gid)))
        {
            child_mode &= !0o6000;
        }
        self.nodes[slot] = Node {
            name: encoded,
            parent,
            kind,
            uid: owner.uid,
            gid: if inherit_setgid {
                parent_gid
            } else {
                owner.gid
            },
            mode: child_mode,
            occupied: true,
        };
        self.contents[slot].fill(0);
        self.content_lengths[slot] = 0;
        if slot == self.len {
            self.len += 1;
        }
        Ok(())
    }

    fn resolve(&self, path: &str, context: AccessContext<'_>) -> Result<usize, VfsError> {
        if !path.starts_with('/') || N == 0 {
            return Err(VfsError::InvalidPath);
        }
        // Mounted Linux files can have longer names and characters such as
        // '+' in libstdc++.so.6. Search each overlay directory before falling
        // through to the mounted filesystem.
        let on_mount = self.mount_for(path).is_some();
        if path
            .split('/')
            .filter(|part| !part.is_empty() && *part != "." && *part != "..")
            .any(|part| {
                (part.len() >= NAME_MAX && !on_mount)
                    || !part.bytes().all(|b| {
                        b.is_ascii_alphanumeric() || b"._-".contains(&b) || (on_mount && b == b'+')
                    })
            })
        {
            return Err(VfsError::InvalidPath);
        }
        let mut current = 0;
        for component in path.split('/').filter(|part| !part.is_empty()) {
            if self.nodes[current].kind != NodeKind::Directory {
                return Err(VfsError::NotDirectory);
            }
            self.check_node_access(current, context, Access::EXECUTE)?;
            if component == "." {
                continue;
            }
            if component == ".." {
                current = self.nodes[current].parent;
                continue;
            }
            if component.len() >= NAME_MAX {
                return Err(VfsError::NotFound);
            }
            current = self
                .find_child(current, component.as_bytes())
                .ok_or(VfsError::NotFound)?;
        }
        Ok(current)
    }

    fn check_node_access(
        &self,
        index: usize,
        context: AccessContext<'_>,
        requested: Access,
    ) -> Result<(), VfsError> {
        let node = self.nodes[index];
        let credentials = context.credentials;
        if credentials.uid == 0 {
            // Linux root bypasses read/write checks; executing a regular file
            // still requires at least one execute bit.
            if requested.contains(Access::EXECUTE)
                && node.kind == NodeKind::File
                && node.mode & 0o111 == 0
            {
                return Err(VfsError::PermissionDenied);
            }
            return Ok(());
        }
        let shift = if credentials.uid == node.uid {
            6
        } else if context.in_group(node.gid) {
            3
        } else {
            0
        };
        if Access(((node.mode >> shift) & 7) as u8).contains(requested) {
            Ok(())
        } else {
            Err(VfsError::PermissionDenied)
        }
    }

    fn find_child(&self, parent: usize, name: &[u8]) -> Option<usize> {
        self.nodes[..self.len]
            .iter()
            .enumerate()
            .find_map(|(i, node)| {
                (node.occupied
                    && node.parent == parent
                    && node.name[..name.len()] == *name
                    && node.name[name.len()] == 0)
                    .then_some(i)
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metadata {
    pub inode: u64,
    pub kind: NodeKind,
    pub uid: u32,
    pub gid: u32,
    pub mode: u16,
    pub size: u64,
}

impl<const N: usize> Default for Vfs<N> {
    fn default() -> Self {
        Self::new()
    }
}

fn snapshot_crc(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for (index, byte) in bytes.iter().enumerate() {
        let value = if (16..20).contains(&index) { 0 } else { *byte };
        crc ^= value as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

fn valid_mount_path(path: &str) -> bool {
    if path == "/" {
        return true;
    }
    if path.is_empty()
        || !path.starts_with('/')
        || path.len() >= MOUNT_PATH_MAX
        || path.ends_with('/')
    {
        return false;
    }
    path.split('/').skip(1).all(|part| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-+".contains(&b))
    })
}

fn path_matches_mount(path: &str, mount: &[u8]) -> bool {
    let Ok(mount) = core::str::from_utf8(mount) else {
        return false;
    };
    path == mount
        || (mount == "/" && path.starts_with('/'))
        || (path.starts_with(mount) && path.as_bytes().get(mount.len()) == Some(&b'/'))
}

impl Access {
    const WRITE_EXECUTE: Self = Self(3);
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: Credentials = Credentials { uid: 0, gid: 0 };
    const ALICE: Credentials = Credentials {
        uid: 1000,
        gid: 1000,
    };
    const BOB: Credentials = Credentials {
        uid: 1001,
        gid: 1001,
    };

    struct TestReadBackend;
    impl ReadOnlyMountBackend for TestReadBackend {
        fn filesystem_type(&self) -> FilesystemType {
            FilesystemType::Ext4
        }
        fn metadata(&mut self, path: &str) -> Result<MountMetadata, VfsError> {
            let (kind, mode) = match path {
                "/" => (NodeKind::Directory, 0o755),
                "/restricted" => (NodeKind::Directory, 0o710),
                "/hello" | "/restricted/hello" => (NodeKind::File, 0o755),
                _ => return Err(VfsError::NotFound),
            };
            Ok(MountMetadata {
                inode: 1,
                kind,
                uid: ROOT.uid,
                gid: 42,
                mode,
                size: 2,
            })
        }

        fn read_file(&mut self, path: &str, output: &mut [u8]) -> Result<usize, VfsError> {
            if path != "/hello" && path != "/restricted/hello" {
                return Err(VfsError::NotFound);
            }
            if output.len() < 2 {
                return Err(VfsError::BufferTooSmall);
            }
            output[..2].copy_from_slice(b"oi");
            Ok(2)
        }
    }

    #[test]
    fn mounted_names_fall_through_without_skipping_overlay_permissions() {
        let mut vfs = Vfs::<8>::new();
        let path = "/libsamba3-util-private-samba.so";
        let protected = "/restricted/libsamba3-util-private-samba.so";
        let alice = AccessContext::new(ALICE, &[], 0);
        assert_eq!(vfs.metadata(path, alice), Err(VfsError::InvalidPath));
        vfs.create_dir("/", "restricted", ROOT, 0o700).unwrap();
        vfs.mount(
            "root",
            "/",
            FilesystemType::Ext4,
            MountOptions {
                read_only: true,
                no_exec: false,
                no_suid: true,
            },
            ROOT,
        )
        .unwrap();
        assert_eq!(vfs.metadata(path, alice), Err(VfsError::NotFound));
        assert_eq!(
            vfs.metadata(protected, alice),
            Err(VfsError::PermissionDenied)
        );
        assert_eq!(
            vfs.metadata("/libstdc++.so.6", alice),
            Err(VfsError::NotFound)
        );
        assert_eq!(
            vfs.metadata("/restricted/libstdc++.so.6", alice),
            Err(VfsError::PermissionDenied)
        );
    }

    #[test]
    fn projected_directory_preserves_mounted_owner_and_mode() {
        let mut vfs = Vfs::<8>::new();
        vfs.mount(
            "root",
            "/",
            FilesystemType::Ext4,
            MountOptions {
                read_only: true,
                ..MountOptions::DEFAULT
            },
            ROOT,
        )
        .unwrap();
        let user = AccessContext::new(ALICE, &[], 0);
        let member = AccessContext::new(ALICE, &[42], 0);
        let mut backend = TestReadBackend;
        assert_eq!(
            vfs.materialize_mounted_directory("/restricted", user, &mut backend),
            Err(VfsError::PermissionDenied)
        );
        assert_eq!(vfs.metadata("/restricted", user), Err(VfsError::NotFound));
        vfs.materialize_mounted_directory("/restricted", member, &mut backend)
            .unwrap();
        let metadata = vfs.metadata("/restricted", member).unwrap();
        assert_eq!((metadata.uid, metadata.gid, metadata.mode), (0, 42, 0o710));
        assert_eq!(
            vfs.create_file_as("/restricted", "new", member, 0o600),
            Err(VfsError::PermissionDenied)
        );
    }

    #[test]
    fn mounted_reads_are_routed_to_the_typed_backend() {
        let mut vfs = Vfs::<8>::new();
        vfs.create_dir("/", "mnt", ROOT, 0o755).unwrap();
        vfs.mount(
            "/dev/test",
            "/mnt",
            FilesystemType::Ext4,
            MountOptions {
                read_only: true,
                ..MountOptions::DEFAULT
            },
            ROOT,
        )
        .unwrap();
        let context = AccessContext::new(ALICE, &[], 0);
        let mut backend = TestReadBackend;
        let mut output = [0; 4];
        assert_eq!(
            vfs.read_mounted_file("/mnt/hello", context, &mut backend, &mut output),
            Ok(2)
        );
        assert_eq!(&output[..2], b"oi");
        let restricted = "/mnt/restricted/hello";
        assert_eq!(
            vfs.mounted_metadata(restricted, context, &mut backend),
            Err(VfsError::PermissionDenied)
        );
        assert_eq!(
            vfs.read_mounted_file_range(restricted, context, &mut backend, 0, &mut output),
            Err(VfsError::PermissionDenied)
        );
        assert_eq!(
            vfs.authorize_mounted_execution(restricted, context, &mut backend),
            Err(VfsError::PermissionDenied)
        );
        let supplementary = AccessContext::new(ALICE, &[42], 0);
        assert_eq!(
            vfs.read_mounted_file(restricted, supplementary, &mut backend, &mut output),
            Ok(2)
        );
        assert_eq!(
            vfs.authorize_mounted_execution(restricted, supplementary, &mut backend),
            Ok(())
        );
    }

    #[test]
    fn vfs_snapshot_round_trips_nested_files_and_rejects_corruption() {
        let mut original = Vfs::<8>::new();
        original.create_dir("/", "etc", ROOT, 0o755).unwrap();
        original.create_file("/etc", "config", ROOT, 0o640).unwrap();
        let root = AccessContext::new(ROOT, &[], 0);
        original
            .write_file_as("/etc/config", root, b"persistent config")
            .unwrap();
        let mut bytes = [0u8; 4096];
        let length = original.encode_snapshot(&mut bytes).unwrap();
        let restored = Vfs::<8>::decode_snapshot(&bytes[..length]).unwrap();
        let mut output = [0; 32];
        assert_eq!(
            restored.read_file_as("/etc/config", root, &mut output),
            Ok(17)
        );
        assert_eq!(&output[..17], b"persistent config");
        bytes[length - 1] ^= 0x80;
        assert!(matches!(
            Vfs::<8>::decode_snapshot(&bytes[..length]),
            Err(VfsError::InvalidPath)
        ));
    }

    #[test]
    fn large_file_ranges_and_snapshot_round_trip() {
        let mut fs = Vfs::<4>::new();
        fs.create_file("/", "large", ROOT, 0o644).unwrap();
        let owner = AccessContext::new(ROOT, &[], 0);
        let bytes = [0x5au8; VFS_INLINE_FILE_MAX];
        assert_eq!(
            fs.write_file_range_as("/large", owner, 0, &bytes),
            Ok(bytes.len())
        );
        assert_eq!(
            fs.file_len_as("/large", owner, Access::READ),
            Ok(bytes.len())
        );
        let mut snapshot = [0u8; 20 + 4 * (18 + NAME_MAX + VFS_INLINE_FILE_MAX)];
        let length = fs.encode_snapshot(&mut snapshot).unwrap();
        let mut restored = Vfs::<4>::new();
        restored.restore_snapshot(&snapshot[..length]).unwrap();
        let mut tail = [0; 32];
        assert_eq!(
            restored.read_file_range_as("/large", owner, bytes.len() - 32, &mut tail),
            Ok(32)
        );
        assert_eq!(tail, [0x5a; 32]);
    }

    #[test]
    fn creates_modular_system_layout_and_checks_owner_group_other_bits() {
        let mut fs = Vfs::<32>::new();
        fs.create_system_layout().unwrap();
        fs.set_mode("/programas", ROOT, 0o777).unwrap();
        fs.create_file("/programas", "editor", ALICE, 0o750)
            .unwrap();
        assert_eq!(fs.authorize_execution("/programas/editor", ALICE), Ok(()));
        assert_eq!(
            fs.authorize_execution("/programas/editor", BOB),
            Err(VfsError::PermissionDenied)
        );
        assert_eq!(
            fs.check_access("/configuracoes/servicos", BOB, Access::READ),
            Err(VfsError::PermissionDenied)
        );
        assert_eq!(
            fs.check_access("/dados", BOB, Access::EXECUTE),
            Ok(NodeKind::Directory)
        );
    }

    #[test]
    fn file_reads_and_writes_enforce_permissions_and_capacity() {
        let mut fs = Vfs::<8>::new();
        fs.create_dir("/", "dados", ROOT, 0o755).unwrap();
        fs.set_mode("/dados", ROOT, 0o777).unwrap();
        fs.create_file("/dados", "nota", ALICE, 0o600).unwrap();
        let owner = AccessContext::new(ALICE, &[], 0);
        fs.write_file_as("/dados/nota", owner, b"persistencia pequena")
            .unwrap();
        let mut bytes = [0; 32];
        let length = fs.read_file_as("/dados/nota", owner, &mut bytes).unwrap();
        assert_eq!(&bytes[..length], b"persistencia pequena");

        let outsider = AccessContext::new(BOB, &[], 0);
        assert_eq!(
            fs.read_file_as("/dados/nota", outsider, &mut bytes),
            Err(VfsError::PermissionDenied)
        );
        assert_eq!(
            fs.write_file_as("/dados/nota", owner, &[0; VFS_INLINE_FILE_MAX + 1]),
            Err(VfsError::FileTooLarge)
        );
        assert_eq!(
            fs.read_file_as("/dados/", owner, &mut bytes),
            Err(VfsError::IsDirectory)
        );
    }

    #[test]
    fn ranged_io_preserves_offsets_checks_access_and_truncates() {
        let mut fs = Vfs::<8>::new();
        fs.create_dir("/", "tmp", ROOT, 0o777).unwrap();
        fs.create_file("/tmp", "data", ALICE, 0o600).unwrap();
        let owner = AccessContext::new(ALICE, &[], 0);
        fs.write_file_as("/tmp/data", owner, b"abcdef").unwrap();
        let mut part = [0; 3];
        assert_eq!(
            fs.read_file_range_as("/tmp/data", owner, 2, &mut part),
            Ok(3)
        );
        assert_eq!(&part, b"cde");
        assert_eq!(fs.write_file_range_as("/tmp/data", owner, 3, b"XY"), Ok(2));
        let mut full = [0; 16];
        let length = fs.read_file_as("/tmp/data", owner, &mut full).unwrap();
        assert_eq!(&full[..length], b"abcXYf");
        assert_eq!(fs.write_file_range_as("/tmp/data", owner, 8, b"z"), Ok(1));
        let length = fs.read_file_as("/tmp/data", owner, &mut full).unwrap();
        assert_eq!(&full[..length], b"abcXYf\0\0z");
        fs.truncate_file_as("/tmp/data", owner).unwrap();
        assert_eq!(fs.file_len_as("/tmp/data", owner, Access::READ), Ok(0));
        assert_eq!(
            fs.write_file_range_as("/tmp/data", AccessContext::new(BOB, &[], 0), 0, b"x"),
            Err(VfsError::PermissionDenied)
        );
    }

    #[test]
    fn unlink_checks_parent_permissions_sticky_bit_and_reuses_slot() {
        let mut fs = Vfs::<4>::new();
        fs.create_dir("/", "tmp", ROOT, 0o1777).unwrap();
        let alice = AccessContext::new(ALICE, &[], 0);
        let bob = AccessContext::new(BOB, &[], 0);
        fs.create_file_as("/tmp", "a", alice, 0o600).unwrap();
        assert_eq!(fs.remove_file_as("/tmp/a", bob), Err(VfsError::NotOwner));
        assert_eq!(
            fs.remove_file_unless_open_as("/tmp/./a", bob, ["/tmp/a"]),
            Err(VfsError::NotOwner)
        );
        assert_eq!(
            fs.remove_file_unless_open_as("/tmp/./a", alice, ["//tmp/../tmp/a"]),
            Err(VfsError::Busy)
        );
        assert_eq!(
            fs.remove_file_unless_open_as("/tmp/a", alice, ["/tmp/inexistente/../a"]),
            Ok(())
        );
        fs.create_file_as("/tmp", "b", bob, 0o600).unwrap();
        assert_eq!(fs.len, 3);
        assert_eq!(fs.metadata("/tmp/b", bob).unwrap().uid, BOB.uid);
        assert_eq!(
            fs.remove_file_as("/tmp", AccessContext::new(ROOT, &[], 0)),
            Err(VfsError::IsDirectory)
        );
    }

    #[test]
    fn rejects_duplicate_names_and_non_owner_mode_changes() {
        let mut fs = Vfs::<8>::new();
        fs.create_dir("/", "programas", ROOT, 0o777).unwrap();
        assert_eq!(
            fs.create_dir("/", "programas", ROOT, 0o755),
            Err(VfsError::AlreadyExists)
        );
        fs.create_file("/programas", "app", ALICE, 0o700).unwrap();
        assert_eq!(
            fs.set_mode("/programas/app", BOB, 0o755),
            Err(VfsError::NotOwner)
        );
        assert_eq!(fs.set_mode("/programas/app", ALICE, 0o750), Ok(()));
    }

    #[test]
    fn supplementary_groups_umask_and_setgid_follow_unix_rules() {
        let staff = Credentials {
            uid: 1000,
            gid: 1000,
        };
        let mut fs = Vfs::<12>::new();
        fs.create_dir("/", "compartilhado", ROOT, 0o2770).unwrap();
        fs.set_owner("/compartilhado", ROOT, 0, 1000).unwrap();
        // `chown` remove setgid conforme POSIX; restaure após transferir grupo.
        fs.set_mode("/compartilhado", ROOT, 0o2770).unwrap();
        let staff_ctx = AccessContext::new(staff, &[], 0);
        fs.create_file_as("/compartilhado", "relatorio", staff_ctx, 0o666)
            .unwrap();
        let meta = fs.metadata("/compartilhado/relatorio", staff_ctx).unwrap();
        assert_eq!((meta.uid, meta.gid, meta.mode), (1000, 1000, 0o666));

        let member = Credentials {
            uid: 1002,
            gid: 1002,
        };
        let group_ctx = AccessContext::new(member, &[1000], 0o027);
        assert_eq!(
            fs.check_access_as("/compartilhado/relatorio", group_ctx, Access::WRITE),
            Ok(NodeKind::File)
        );
        fs.create_file_as("/compartilhado", "private", group_ctx, 0o666)
            .unwrap();
        let private = fs.metadata("/compartilhado/private", group_ctx).unwrap();
        assert_eq!((private.gid, private.mode), (1000, 0o640));
        fs.create_dir_as("/compartilhado", "subdir", group_ctx, 0o750)
            .unwrap();
        let subdir = fs.metadata("/compartilhado/subdir", group_ctx).unwrap();
        assert_eq!((subdir.gid, subdir.mode), (1000, 0o2750));

        let outsider = AccessContext::new(BOB, &[], 0);
        assert_eq!(
            fs.check_access_as("/compartilhado/relatorio", outsider, Access::READ),
            Err(VfsError::PermissionDenied)
        );
    }

    #[test]
    fn mounts_require_root_and_longest_path_wins() {
        let mut fs = Vfs::<12>::new();
        fs.create_dir("/", "mnt", ROOT, 0o755).unwrap();
        fs.create_dir("/mnt", "data", ROOT, 0o755).unwrap();
        fs.mount(
            "/dev/sda1",
            "/mnt",
            FilesystemType::Fat32,
            MountOptions {
                read_only: true,
                no_exec: true,
                no_suid: true,
            },
            ROOT,
        )
        .unwrap();
        fs.mount(
            "proc",
            "/mnt/data",
            FilesystemType::Proc,
            MountOptions::DEFAULT,
            ROOT,
        )
        .unwrap();
        assert_eq!(
            fs.mount_for("/mnt/file").unwrap().filesystem,
            FilesystemType::Fat32
        );
        assert_eq!(
            fs.mount_for("/mnt/data/status").unwrap().filesystem,
            FilesystemType::Proc
        );
        assert_eq!(fs.mount_for("/mnt2/file"), None);
        assert_eq!(
            fs.mount(
                "x",
                "/mnt",
                FilesystemType::Fat32,
                MountOptions::DEFAULT,
                ROOT
            ),
            Err(VfsError::AlreadyMounted)
        );
        assert_eq!(fs.unmount("/mnt", ALICE), Err(VfsError::MountDenied));
        fs.unmount("/mnt/data", ROOT).unwrap();
        assert_eq!(
            fs.mount_for("/mnt/data/status").unwrap().filesystem,
            FilesystemType::Fat32
        );
    }

    #[test]
    fn mount_paths_must_be_canonical_existing_directories() {
        let mut fs = Vfs::<4>::new();
        fs.create_dir("/", "mnt", ROOT, 0o755).unwrap();
        assert_eq!(
            fs.mount(
                "disk",
                "/missing",
                FilesystemType::Hermione,
                MountOptions::DEFAULT,
                ROOT
            ),
            Err(VfsError::NotFound)
        );
        assert_eq!(
            fs.mount(
                "disk",
                "/mnt/../tmp",
                FilesystemType::Hermione,
                MountOptions::DEFAULT,
                ROOT
            ),
            Err(VfsError::InvalidMountPath)
        );
    }

    #[test]
    fn root_mount_covers_absolute_paths_and_allows_nested_mounts() {
        let mut fs = Vfs::<4>::new();
        fs.create_dir("/", "mnt", ROOT, 0o755).unwrap();
        fs.mount(
            "root",
            "/",
            FilesystemType::Ext4,
            MountOptions::DEFAULT,
            ROOT,
        )
        .unwrap();
        fs.mount(
            "removable",
            "/mnt",
            FilesystemType::Fat32,
            MountOptions::DEFAULT,
            ROOT,
        )
        .unwrap();
        assert_eq!(fs.mount_for("/").unwrap().source, "root");
        assert_eq!(fs.mount_for("/bin/app").unwrap().source, "root");
        assert_eq!(fs.mount_for("/mnt/file").unwrap().source, "removable");
    }

    #[test]
    fn creates_fhs_paths_for_linux_userlands() {
        let mut fs = Vfs::<32>::new();
        fs.create_fhs_layout().unwrap();
        for path in [
            "/etc", "/usr/bin", "/usr/lib", "/var/lib", "/var/log", "/tmp",
        ] {
            assert_eq!(
                fs.check_access(path, ROOT, Access::EXECUTE),
                Ok(NodeKind::Directory)
            );
        }
        assert_eq!(
            fs.metadata("/tmp", AccessContext::new(ROOT, &[], 0))
                .unwrap()
                .mode,
            0o1777
        );
        assert_eq!(
            fs.metadata("/root", AccessContext::new(ROOT, &[], 0))
                .unwrap()
                .mode,
            0o700
        );
    }

    #[test]
    fn root_can_bypass_read_write_but_not_missing_execute_bit() {
        let mut fs = Vfs::<8>::new();
        fs.create_dir("/", "programas", ROOT, 0o755).unwrap();
        fs.create_file("/programas", "sem_exec", ROOT, 0o600)
            .unwrap();
        fs.create_file("/programas", "executavel", ROOT, 0o100)
            .unwrap();
        assert_eq!(
            fs.check_access("/programas/sem_exec", ROOT, Access::READ),
            Ok(NodeKind::File)
        );
        assert_eq!(
            fs.authorize_execution("/programas/sem_exec", ROOT),
            Err(VfsError::PermissionDenied)
        );
        assert_eq!(
            fs.authorize_execution("/programas/executavel", ROOT),
            Ok(())
        );
    }

    #[test]
    fn only_root_can_change_owner_and_metadata_preserves_special_mode_bits() {
        let mut fs = Vfs::<8>::new();
        fs.create_dir("/", "dados", ROOT, 0o755).unwrap();
        fs.create_file("/dados", "arquivo", ROOT, 0o640).unwrap();
        fs.set_owner("/dados/arquivo", ROOT, ALICE.uid, ALICE.gid)
            .unwrap();
        assert_eq!(
            fs.set_owner("/dados/arquivo", ALICE, 2000, 2000),
            Err(VfsError::NotOwner)
        );
        fs.set_owner("/dados/arquivo", ROOT, 2000, 2001).unwrap();
        fs.set_mode("/dados/arquivo", ROOT, 0o4750).unwrap();
        let meta = fs
            .metadata("/dados/arquivo", AccessContext::new(ROOT, &[], 0))
            .unwrap();
        assert_eq!((meta.uid, meta.gid, meta.mode), (2000, 2001, 0o4750));
    }

    #[test]
    fn search_permission_is_required_for_dot_and_parent_components() {
        let mut fs = Vfs::<8>::new();
        fs.create_dir("/", "locked", ROOT, 0o700).unwrap();
        fs.create_file("/locked", "inside", ROOT, 0o644).unwrap();
        fs.set_mode("/locked", ROOT, 0o600).unwrap();
        assert_eq!(
            fs.check_access("/locked/inside", ALICE, Access::READ),
            Err(VfsError::PermissionDenied)
        );
        assert_eq!(
            fs.check_access("/locked/..", ALICE, Access::READ),
            Err(VfsError::PermissionDenied)
        );
        assert_eq!(
            fs.check_access("/locked/.", ALICE, Access::READ),
            Err(VfsError::PermissionDenied)
        );
    }
}

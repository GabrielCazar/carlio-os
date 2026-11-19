//! Validador e carregador de imagens ELF64 little-endian para x86_64.

pub const ELF_HEADER_SIZE: usize = 64;
pub const PROGRAM_HEADER_SIZE: usize = 56;
pub const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;
const DT_NULL: i64 = 0;
const DT_NEEDED: i64 = 1;
const DT_STRTAB: i64 = 5;
const DT_STRSZ: i64 = 10;
const DT_RELA: i64 = 7;
const DT_RELASZ: i64 = 8;
const DT_RELAENT: i64 = 9;
const DT_RELACOUNT: i64 = 0x6fff_fff9;
const DT_PLTRELSZ: i64 = 2;
const DT_REL: i64 = 17;
const DT_RELSZ: i64 = 18;
const DT_RELENT: i64 = 19;
const DT_JMPREL: i64 = 23;
const R_X86_64_RELATIVE: u32 = 8;
pub const PF_X: u32 = 1;
pub const PF_W: u32 = 2;
pub const PF_R: u32 = 4;
pub const USER_TOP: u64 = 0x0000_8000_0000_0000;
pub const PAGE_SIZE: u64 = 4096;
const MAX_LOAD_PAGES: usize = 1024;
const ELF_NAME_MAX: usize = 128;
const PT_PHDR: u32 = 6;
pub const AT_NULL: u64 = 0;
pub const AT_PHDR: u64 = 3;
pub const AT_PHENT: u64 = 4;
pub const AT_PHNUM: u64 = 5;
pub const AT_PAGESZ: u64 = 6;
pub const AT_BASE: u64 = 7;
pub const AT_ENTRY: u64 = 9;
const DYNAMIC_INTERPRETER_BIAS: u64 = 0x0000_7000_0000_0000;
const DYNAMIC_PROGRAM_BIAS: u64 = 0x0000_0040_0000_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElfError {
    Truncated,
    InvalidMagic,
    UnsupportedFormat,
    InvalidHeader,
    TooManySegments,
    InvalidSegment,
    OverlappingSegments,
    EntryNotExecutable,
    MappingFailed,
    InvalidArguments,
    UnsupportedRelocation,
    DynamicMetadataInvalid,
    TooManyDependencies,
}

/// Metadados bounded para o próximo estágio de exec: interpretador ELF e
/// `DT_NEEDED`. Os nomes permanecem em buffers próprios para uso `no_std`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ElfDynamicMetadata<const N: usize> {
    interpreter: [u8; ELF_NAME_MAX],
    interpreter_len: usize,
    dependencies: [[u8; ELF_NAME_MAX]; N],
    dependency_lengths: [u8; N],
    dependency_count: usize,
}

impl<const N: usize> ElfDynamicMetadata<N> {
    pub fn parse(bytes: &[u8]) -> Result<Self, ElfError> {
        if bytes.len() < ELF_HEADER_SIZE || &bytes[..4] != b"\x7fELF" {
            return Err(ElfError::InvalidHeader);
        }
        if bytes[4] != 2 || bytes[5] != 1 || u16le(bytes, 18)? != 62 {
            return Err(ElfError::UnsupportedFormat);
        }
        let phoff = usize::try_from(u64le(bytes, 32)?).map_err(|_| ElfError::InvalidHeader)?;
        let phnum = u16le(bytes, 56)? as usize;
        let phbytes = phnum
            .checked_mul(PROGRAM_HEADER_SIZE)
            .ok_or(ElfError::InvalidHeader)?;
        if phoff
            .checked_add(phbytes)
            .is_none_or(|end| end > bytes.len())
        {
            return Err(ElfError::Truncated);
        }
        let mut result = Self {
            interpreter: [0; ELF_NAME_MAX],
            interpreter_len: 0,
            dependencies: [[0; ELF_NAME_MAX]; N],
            dependency_lengths: [0; N],
            dependency_count: 0,
        };
        let mut dynamic: Option<(usize, usize)> = None;
        let mut strtab_va = None;
        let mut strtab_size = None;
        let mut needed = [0u32; N];
        let mut needed_count = 0usize;
        let mut loads = [(0u64, 0u64, 0u64, 0u64); 64];
        let mut load_count = 0;
        for i in 0..phnum {
            let p = phoff + i * PROGRAM_HEADER_SIZE;
            match u32le(bytes, p)? {
                PT_LOAD => {
                    if load_count == loads.len() {
                        return Err(ElfError::TooManySegments);
                    }
                    loads[load_count] = (
                        u64le(bytes, p + 16)?,
                        u64le(bytes, p + 8)?,
                        u64le(bytes, p + 32)?,
                        u64le(bytes, p + 40)?,
                    );
                    load_count += 1;
                }
                PT_INTERP => {
                    if result.interpreter_len != 0 {
                        return Err(ElfError::DynamicMetadataInvalid);
                    }
                    let off = usize::try_from(u64le(bytes, p + 8)?)
                        .map_err(|_| ElfError::DynamicMetadataInvalid)?;
                    let size = usize::try_from(u64le(bytes, p + 32)?)
                        .map_err(|_| ElfError::DynamicMetadataInvalid)?;
                    let data = bytes
                        .get(
                            off..off
                                .checked_add(size)
                                .ok_or(ElfError::DynamicMetadataInvalid)?,
                        )
                        .ok_or(ElfError::Truncated)?;
                    if size < 2
                        || data[size - 1] != 0
                        || data[..size - 1].contains(&0)
                        || size > ELF_NAME_MAX
                        || data[0] != b'/'
                    {
                        return Err(ElfError::DynamicMetadataInvalid);
                    }
                    result.interpreter[..size - 1].copy_from_slice(&data[..size - 1]);
                    result.interpreter_len = size - 1;
                }
                PT_DYNAMIC => {
                    if dynamic.is_some() {
                        return Err(ElfError::DynamicMetadataInvalid);
                    }
                    let off = usize::try_from(u64le(bytes, p + 8)?)
                        .map_err(|_| ElfError::DynamicMetadataInvalid)?;
                    let size = usize::try_from(u64le(bytes, p + 32)?)
                        .map_err(|_| ElfError::DynamicMetadataInvalid)?;
                    if size % 16 != 0
                        || bytes
                            .get(
                                off..off
                                    .checked_add(size)
                                    .ok_or(ElfError::DynamicMetadataInvalid)?,
                            )
                            .is_none()
                    {
                        return Err(ElfError::Truncated);
                    }
                    dynamic = Some((off, size));
                }
                _ => {}
            }
        }
        if let Some((offset, size)) = dynamic {
            let mut terminated = false;
            for entry in bytes[offset..offset + size].as_chunks::<16>().0 {
                let tag = i64::from_le_bytes(entry[..8].try_into().unwrap());
                let value = u64::from_le_bytes(entry[8..].try_into().unwrap());
                if tag == DT_NULL {
                    terminated = true;
                    break;
                }
                match tag {
                    DT_NEEDED => {
                        if needed_count == N {
                            return Err(ElfError::TooManyDependencies);
                        }
                        needed[needed_count] = value
                            .try_into()
                            .map_err(|_| ElfError::DynamicMetadataInvalid)?;
                        needed_count += 1;
                    }
                    DT_STRTAB => {
                        if strtab_va.replace(value).is_some() {
                            return Err(ElfError::DynamicMetadataInvalid);
                        }
                    }
                    DT_STRSZ => {
                        if strtab_size.replace(value).is_some() {
                            return Err(ElfError::DynamicMetadataInvalid);
                        }
                    }
                    _ => {}
                }
            }
            if !terminated {
                return Err(ElfError::DynamicMetadataInvalid);
            }
            if needed_count != 0 {
                let va = strtab_va.ok_or(ElfError::DynamicMetadataInvalid)?;
                let size = usize::try_from(strtab_size.ok_or(ElfError::DynamicMetadataInvalid)?)
                    .map_err(|_| ElfError::DynamicMetadataInvalid)?;
                let mut file_offset = None;
                for (load_va, file_off, file_sz, _) in &loads[..load_count] {
                    if va >= *load_va
                        && va
                            .checked_add(size as u64)
                            .is_some_and(|end| end <= load_va.saturating_add(*file_sz))
                    {
                        file_offset = Some(
                            file_off
                                .checked_add(va - load_va)
                                .ok_or(ElfError::DynamicMetadataInvalid)?
                                as usize,
                        );
                        break;
                    }
                }
                let start = file_offset.ok_or(ElfError::DynamicMetadataInvalid)?;
                let strings = bytes
                    .get(
                        start
                            ..start
                                .checked_add(size)
                                .ok_or(ElfError::DynamicMetadataInvalid)?,
                    )
                    .ok_or(ElfError::Truncated)?;
                for offset in needed.iter().take(needed_count) {
                    let tail = strings
                        .get(*offset as usize..)
                        .ok_or(ElfError::DynamicMetadataInvalid)?;
                    let len = tail
                        .iter()
                        .position(|byte| *byte == 0)
                        .ok_or(ElfError::DynamicMetadataInvalid)?;
                    if len == 0 || len >= ELF_NAME_MAX || tail[..len].contains(&b'/') {
                        return Err(ElfError::DynamicMetadataInvalid);
                    }
                    result.dependencies[result.dependency_count][..len]
                        .copy_from_slice(&tail[..len]);
                    result.dependency_lengths[result.dependency_count] = len as u8;
                    result.dependency_count += 1;
                }
            }
        }
        Ok(result)
    }

    pub fn interpreter(&self) -> Option<&str> {
        core::str::from_utf8(&self.interpreter[..self.interpreter_len])
            .ok()
            .filter(|s| !s.is_empty())
    }

    pub fn dependencies(&self) -> impl Iterator<Item = &str> {
        self.dependencies[..self.dependency_count]
            .iter()
            .zip(&self.dependency_lengths[..self.dependency_count])
            .filter_map(|(name, len)| core::str::from_utf8(&name[..*len as usize]).ok())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserStack {
    pub stack_pointer: u64,
    pub argc: u64,
    pub argv: u64,
    pub envp: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuxEntry {
    pub kind: u64,
    pub value: u64,
}

/// Endereços necessários para transferir o controle ao interpretador dinâmico.
/// `entry` é o início do ld-linux; `program_entry` é o AT_ENTRY que ele iniciará.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecImage {
    pub entry: u64,
    pub program_entry: u64,
    pub interpreter_base: u64,
    pub program_headers: u64,
    pub program_header_entry_size: u64,
    pub program_header_count: u64,
    /// Page-aligned enclosing ranges of the main image and optional loader.
    pub image_ranges: [(u64, u64); 2],
}

impl ExecImage {
    pub fn auxv(self) -> [AuxEntry; 6] {
        [
            AuxEntry {
                kind: AT_PHDR,
                value: self.program_headers,
            },
            AuxEntry {
                kind: AT_PHENT,
                value: self.program_header_entry_size,
            },
            AuxEntry {
                kind: AT_PHNUM,
                value: self.program_header_count,
            },
            AuxEntry {
                kind: AT_PAGESZ,
                value: PAGE_SIZE,
            },
            AuxEntry {
                kind: AT_BASE,
                value: self.interpreter_base,
            },
            AuxEntry {
                kind: AT_ENTRY,
                value: self.program_entry,
            },
        ]
    }
}

/// Places NUL-terminated argv/env strings and the initial pointer vectors in
/// an already mapped user stack page. Strings cannot contain interior NULs;
/// the layout follows the SysV process-entry convention.
pub fn build_initial_user_stack<T: ElfLoadTarget>(
    target: &mut T,
    stack_top: u64,
    argv: &[&str],
    env: &[&str],
) -> Result<UserStack, ElfError> {
    build_initial_user_stack_with_auxv(target, stack_top, argv, env, &[])
}

/// Monta a pilha inicial SysV (`argc/argv/envp/auxv`) para executável ELF.
pub fn build_initial_user_stack_with_auxv<T: ElfLoadTarget>(
    target: &mut T,
    stack_top: u64,
    argv: &[&str],
    env: &[&str],
    auxv: &[AuxEntry],
) -> Result<UserStack, ElfError> {
    build_initial_user_stack_with_auxv_sized(target, stack_top, 4096, argv, env, auxv)
}

/// Variante com tamanho explícito para stacks com mais de uma página.
pub fn build_initial_user_stack_with_auxv_sized<T: ElfLoadTarget>(
    target: &mut T,
    stack_top: u64,
    stack_bytes: u64,
    argv: &[&str],
    env: &[&str],
    auxv: &[AuxEntry],
) -> Result<UserStack, ElfError> {
    const STACK_BYTES_MIN: u64 = 4096;
    const VECTOR_LIMIT: usize = 16;
    if argv.len() > VECTOR_LIMIT || env.len() > VECTOR_LIMIT {
        return Err(ElfError::InvalidArguments);
    }
    if !(STACK_BYTES_MIN..=USER_TOP).contains(&stack_top)
        || stack_top & 0xf != 0
        || stack_bytes < STACK_BYTES_MIN
        || stack_bytes & 0xfff != 0
        || stack_bytes > stack_top
    {
        return Err(ElfError::InvalidArguments);
    }
    let lower_bound = stack_top - stack_bytes;
    let mut sp = stack_top;
    let mut arg_ptrs = [0u64; VECTOR_LIMIT];
    let mut env_ptrs = [0u64; VECTOR_LIMIT];

    for (index, value) in argv.iter().enumerate().rev() {
        sp = push_user_string(target, sp, lower_bound, value)?;
        arg_ptrs[index] = sp;
    }
    for (index, value) in env.iter().enumerate().rev() {
        sp = push_user_string(target, sp, lower_bound, value)?;
        env_ptrs[index] = sp;
    }

    if auxv.len() > 32 || auxv.iter().any(|entry| entry.kind == AT_NULL) {
        return Err(ElfError::InvalidArguments);
    }
    let word_count = 1usize
        .checked_add(argv.len())
        .and_then(|v| v.checked_add(1))
        .and_then(|v| v.checked_add((auxv.len() + 1).checked_mul(2)?))
        .and_then(|v| v.checked_add(env.len()))
        .and_then(|v| v.checked_add(1))
        .ok_or(ElfError::InvalidArguments)?;
    let vector_bytes = (word_count as u64)
        .checked_mul(8)
        .ok_or(ElfError::InvalidArguments)?;
    let vector_start = sp
        .checked_sub(vector_bytes)
        .map(|value| value & !0xf)
        .ok_or(ElfError::InvalidArguments)?;
    if vector_start < lower_bound {
        return Err(ElfError::InvalidArguments);
    }

    let argc = u64::try_from(argv.len()).map_err(|_| ElfError::InvalidArguments)?;
    let argv_address = vector_start + 8;
    let envp_offset = u64::try_from(argv.len() + 1)
        .ok()
        .and_then(|count| count.checked_mul(8))
        .ok_or(ElfError::InvalidArguments)?;
    let envp_address = argv_address
        .checked_add(envp_offset)
        .ok_or(ElfError::InvalidArguments)?;
    let mut cursor = vector_start;
    write_user_word(target, &mut cursor, argc)?;
    for pointer in arg_ptrs.iter().take(argv.len()) {
        write_user_word(target, &mut cursor, *pointer)?;
    }
    write_user_word(target, &mut cursor, 0)?;
    for pointer in env_ptrs.iter().take(env.len()) {
        write_user_word(target, &mut cursor, *pointer)?;
    }
    write_user_word(target, &mut cursor, 0)?;
    for entry in auxv {
        write_user_word(target, &mut cursor, entry.kind)?;
        write_user_word(target, &mut cursor, entry.value)?;
    }
    write_user_word(target, &mut cursor, AT_NULL)?;
    write_user_word(target, &mut cursor, 0)?;
    Ok(UserStack {
        stack_pointer: vector_start,
        argc,
        argv: argv_address,
        envp: envp_address,
    })
}

fn push_user_string<T: ElfLoadTarget>(
    target: &mut T,
    stack_pointer: u64,
    lower_bound: u64,
    value: &str,
) -> Result<u64, ElfError> {
    if value.as_bytes().contains(&0) {
        return Err(ElfError::InvalidArguments);
    }
    let length = (value.len() as u64)
        .checked_add(1)
        .ok_or(ElfError::InvalidArguments)?;
    let address = stack_pointer
        .checked_sub(length)
        .ok_or(ElfError::InvalidArguments)?;
    if address < lower_bound {
        return Err(ElfError::InvalidArguments);
    }
    target
        .write(address, value.as_bytes())
        .map_err(|_| ElfError::MappingFailed)?;
    target
        .write(address + value.len() as u64, &[0])
        .map_err(|_| ElfError::MappingFailed)?;
    Ok(address)
}

fn write_user_word<T: ElfLoadTarget>(
    target: &mut T,
    cursor: &mut u64,
    value: u64,
) -> Result<(), ElfError> {
    target
        .write(*cursor, &value.to_le_bytes())
        .map_err(|_| ElfError::MappingFailed)?;
    *cursor = cursor.checked_add(8).ok_or(ElfError::InvalidArguments)?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VfsElfError {
    Vfs(crate::vfs::VfsError),
    Elf(ElfError),
}

/// Resolve, authorize, read and load an executable through the VFS. The caller
/// owns the scratch buffer and address-space target, so this remains usable in
/// `no_std` without introducing an allocator into the ELF parser.
pub fn load_executable_from_vfs<const S: usize, const N: usize, T: ElfLoadTarget>(
    vfs: &crate::vfs::Vfs<N>,
    path: &str,
    access: crate::vfs::AccessContext<'_>,
    scratch: &mut [u8],
    target: &mut T,
) -> Result<(ElfImage<S>, usize), VfsElfError> {
    vfs.authorize_execution_as(path, access)
        .map_err(VfsElfError::Vfs)?;
    let length = vfs
        .read_file_as(path, access, scratch)
        .map_err(VfsElfError::Vfs)?;
    let image = ElfImage::<S>::parse(&scratch[..length]).map_err(VfsElfError::Elf)?;
    image
        .load_into(&scratch[..length], target)
        .map_err(VfsElfError::Elf)?;
    Ok((image, length))
}

/// Carrega executável e `PT_INTERP` no mesmo espaço de endereços. O kernel
/// apenas mapeia as imagens; o ld-linux é responsável por dependências e
/// relocations dinâmicas. O caminho do interpretador vem do ELF e é validado
/// como caminho absoluto pelo parser e pelo VFS.
pub fn load_interpreted_executable_from_vfs<const S: usize, const N: usize, T: ElfLoadTarget>(
    vfs: &crate::vfs::Vfs<N>,
    path: &str,
    access: crate::vfs::AccessContext<'_>,
    program_scratch: &mut [u8],
    interpreter_scratch: &mut [u8],
    target: &mut T,
) -> Result<ExecImage, VfsElfError> {
    let mut source = TreeExecutableSource { vfs };
    load_interpreted_executable_from_source::<S, _, _>(
        &mut source,
        path,
        access,
        program_scratch,
        interpreter_scratch,
        target,
    )
}

/// Variante que carrega arquivos do namespace montado, com arquivos overlay
/// em RAM tendo precedência sobre o filesystem de base.
pub fn load_interpreted_executable_from_mounted<
    const S: usize,
    const N: usize,
    T: ElfLoadTarget,
    B: crate::vfs::ReadOnlyMountBackend,
>(
    vfs: &crate::vfs::Vfs<N>,
    backend: &mut B,
    path: &str,
    access: crate::vfs::AccessContext<'_>,
    program_scratch: &mut [u8],
    interpreter_scratch: &mut [u8],
    target: &mut T,
) -> Result<ExecImage, VfsElfError> {
    let mut source = MountedExecutableSource { vfs, backend };
    load_interpreted_executable_from_source::<S, _, _>(
        &mut source,
        path,
        access,
        program_scratch,
        interpreter_scratch,
        target,
    )
}

trait ExecutableSource {
    fn authorize(
        &mut self,
        path: &str,
        access: crate::vfs::AccessContext<'_>,
    ) -> Result<(), crate::vfs::VfsError>;
    fn read(
        &mut self,
        path: &str,
        access: crate::vfs::AccessContext<'_>,
        output: &mut [u8],
    ) -> Result<usize, crate::vfs::VfsError>;
}

struct TreeExecutableSource<'a, const N: usize> {
    vfs: &'a crate::vfs::Vfs<N>,
}
impl<const N: usize> ExecutableSource for TreeExecutableSource<'_, N> {
    fn authorize(
        &mut self,
        path: &str,
        access: crate::vfs::AccessContext<'_>,
    ) -> Result<(), crate::vfs::VfsError> {
        self.vfs.authorize_execution_as(path, access)
    }
    fn read(
        &mut self,
        path: &str,
        access: crate::vfs::AccessContext<'_>,
        output: &mut [u8],
    ) -> Result<usize, crate::vfs::VfsError> {
        self.vfs.read_file_as(path, access, output)
    }
}

struct MountedExecutableSource<'a, const N: usize, B> {
    vfs: &'a crate::vfs::Vfs<N>,
    backend: &'a mut B,
}
impl<const N: usize, B: crate::vfs::ReadOnlyMountBackend> ExecutableSource
    for MountedExecutableSource<'_, N, B>
{
    fn authorize(
        &mut self,
        path: &str,
        access: crate::vfs::AccessContext<'_>,
    ) -> Result<(), crate::vfs::VfsError> {
        match self.vfs.authorize_execution_as(path, access) {
            Ok(()) => Ok(()),
            Err(crate::vfs::VfsError::NotFound) => {
                self.vfs
                    .authorize_mounted_execution(path, access, self.backend)
            }
            Err(error) => Err(error),
        }
    }
    fn read(
        &mut self,
        path: &str,
        access: crate::vfs::AccessContext<'_>,
        output: &mut [u8],
    ) -> Result<usize, crate::vfs::VfsError> {
        match self.vfs.read_file_as(path, access, output) {
            Ok(length) => Ok(length),
            Err(crate::vfs::VfsError::NotFound) => {
                self.vfs
                    .read_mounted_file(path, access, self.backend, output)
            }
            Err(error) => Err(error),
        }
    }
}

fn load_interpreted_executable_from_source<
    const S: usize,
    T: ElfLoadTarget,
    F: ExecutableSource,
>(
    source: &mut F,
    path: &str,
    access: crate::vfs::AccessContext<'_>,
    program_scratch: &mut [u8],
    interpreter_scratch: &mut [u8],
    target: &mut T,
) -> Result<ExecImage, VfsElfError> {
    source.authorize(path, access).map_err(VfsElfError::Vfs)?;
    let program_len = source
        .read(path, access, program_scratch)
        .map_err(VfsElfError::Vfs)?;
    let program = &program_scratch[..program_len];
    let metadata = ElfDynamicMetadata::<S>::parse(program).map_err(VfsElfError::Elf)?;
    let interpreter_path = metadata.interpreter();
    let main_bias = if u16le(program, 16).map_err(VfsElfError::Elf)? == 3 {
        DYNAMIC_PROGRAM_BIAS
    } else {
        0
    };
    let program_image = if interpreter_path.is_some() {
        ElfImage::<S>::parse_for_dynamic_loader(program, main_bias).map_err(VfsElfError::Elf)?
    } else {
        ElfImage::<S>::parse_with_load_bias(program, main_bias).map_err(VfsElfError::Elf)?
    };
    if let Some(interpreter_path) = interpreter_path {
        source
            .authorize(interpreter_path, access)
            .map_err(VfsElfError::Vfs)?;
        let interpreter_len = source
            .read(interpreter_path, access, interpreter_scratch)
            .map_err(VfsElfError::Vfs)?;
        let interpreter = &interpreter_scratch[..interpreter_len];
        let interpreter_image =
            ElfImage::<S>::parse_for_dynamic_loader(interpreter, DYNAMIC_INTERPRETER_BIAS)
                .map_err(VfsElfError::Elf)?;
        if !interpreter_image.is_position_independent() {
            return Err(VfsElfError::Elf(ElfError::UnsupportedFormat));
        }
        program_image
            .load_into_unrelocated(program, target)
            .map_err(VfsElfError::Elf)?;
        interpreter_image
            .load_into_unrelocated(interpreter, target)
            .map_err(VfsElfError::Elf)?;
        Ok(ExecImage {
            entry: interpreter_image.entry,
            program_entry: program_image.entry,
            interpreter_base: DYNAMIC_INTERPRETER_BIAS,
            program_headers: program_image.program_headers,
            program_header_entry_size: PROGRAM_HEADER_SIZE as u64,
            program_header_count: program_image.program_header_count,
            image_ranges: [
                program_image.mapped_range().map_err(VfsElfError::Elf)?,
                interpreter_image.mapped_range().map_err(VfsElfError::Elf)?,
            ],
        })
    } else {
        program_image
            .load_into(program, target)
            .map_err(VfsElfError::Elf)?;
        Ok(ExecImage {
            entry: program_image.entry,
            program_entry: program_image.entry,
            interpreter_base: 0,
            program_headers: program_image.program_headers,
            program_header_entry_size: PROGRAM_HEADER_SIZE as u64,
            program_header_count: program_image.program_header_count,
            image_ranges: [
                program_image.mapped_range().map_err(VfsElfError::Elf)?,
                (0, 0),
            ],
        })
    }
}

/// Operações necessárias para carregar segmentos em um espaço de usuário.
/// `map_zeroed_page` deve criar página user NX por padrão; `protect_page`
/// aplica permissões ELF depois da cópia, evitando janelas RWX.
pub trait ElfLoadTarget {
    type Error;
    fn map_zeroed_page(&mut self, page: u64) -> Result<(), Self::Error>;
    fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), Self::Error>;
    fn protect_page(&mut self, page: u64, flags: u32) -> Result<(), Self::Error>;
    fn unmap_page(&mut self, page: u64) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoadSegment {
    pub file_offset: u64,
    pub virtual_address: u64,
    pub file_size: u64,
    pub memory_size: u64,
    pub flags: u32,
    pub alignment: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ElfImage<const N: usize> {
    pub entry: u64,
    segments: [LoadSegment; N],
    segment_count: usize,
    load_bias: u64,
    rela_file_offset: usize,
    rela_count: usize,
    program_headers: u64,
    program_header_count: u64,
    position_independent: bool,
}

impl<const N: usize> ElfImage<N> {
    pub fn parse(bytes: &[u8]) -> Result<Self, ElfError> {
        Self::parse_with_load_bias(bytes, 0)
    }

    /// Aceita ET_EXEC e PIE ET_DYN com bias escolhido pelo kernel. Para PIE,
    /// aplica relocations locais `R_X86_64_RELATIVE`; dependências dinâmicas,
    /// símbolos e PLT continuam sem suporte.
    pub fn parse_with_load_bias(bytes: &[u8], load_bias: u64) -> Result<Self, ElfError> {
        Self::parse_internal(bytes, load_bias, false)
    }

    fn parse_for_dynamic_loader(bytes: &[u8], load_bias: u64) -> Result<Self, ElfError> {
        Self::parse_internal(bytes, load_bias, true)
    }

    fn parse_internal(bytes: &[u8], load_bias: u64, defer_dynamic: bool) -> Result<Self, ElfError> {
        if bytes.len() < ELF_HEADER_SIZE {
            return Err(ElfError::Truncated);
        }
        if &bytes[..4] != b"\x7fELF" {
            return Err(ElfError::InvalidMagic);
        }
        if bytes[4] != 2 || bytes[5] != 1 || bytes[6] != 1 {
            return Err(ElfError::UnsupportedFormat);
        }
        let kind = u16le(bytes, 16)?;
        if (kind != 2 && kind != 3)
            || (kind == 2 && load_bias != 0)
            || (kind == 3 && !load_bias.is_multiple_of(PAGE_SIZE))
            || u16le(bytes, 18)? != 62
            || u32le(bytes, 20)? != 1
        {
            return Err(ElfError::UnsupportedFormat);
        }
        let entry = u64le(bytes, 24)?
            .checked_add(load_bias)
            .ok_or(ElfError::InvalidHeader)?;
        let phoff = usize::try_from(u64le(bytes, 32)?).map_err(|_| ElfError::InvalidHeader)?;
        if u16le(bytes, 52)? as usize != ELF_HEADER_SIZE
            || u16le(bytes, 54)? as usize != PROGRAM_HEADER_SIZE
        {
            return Err(ElfError::InvalidHeader);
        }
        let phnum = u16le(bytes, 56)? as usize;
        let phbytes = phnum
            .checked_mul(PROGRAM_HEADER_SIZE)
            .ok_or(ElfError::InvalidHeader)?;
        let phend = phoff.checked_add(phbytes).ok_or(ElfError::InvalidHeader)?;
        if phend > bytes.len() {
            return Err(ElfError::Truncated);
        }
        let mut segments = [LoadSegment::default(); N];
        let mut count = 0;
        let mut entry_executable = false;
        let mut dynamic: Option<(usize, usize, u64, usize)> = None;
        let mut explicit_phdr = None;
        for index in 0..phnum {
            let p = phoff + index * PROGRAM_HEADER_SIZE;
            let segment_type = u32le(bytes, p)?;
            if segment_type == PT_INTERP {
                if !defer_dynamic {
                    return Err(ElfError::UnsupportedFormat);
                }
                continue;
            }
            if segment_type == PT_PHDR {
                explicit_phdr = Some(
                    u64le(bytes, p + 16)?
                        .checked_add(load_bias)
                        .ok_or(ElfError::InvalidHeader)?,
                );
                continue;
            }
            if segment_type == PT_DYNAMIC {
                if defer_dynamic {
                    continue;
                }
                if dynamic.is_some() {
                    return Err(ElfError::InvalidHeader);
                }
                let offset =
                    usize::try_from(u64le(bytes, p + 8)?).map_err(|_| ElfError::InvalidHeader)?;
                let file_size =
                    usize::try_from(u64le(bytes, p + 32)?).map_err(|_| ElfError::InvalidHeader)?;
                let memory_size = u64le(bytes, p + 40)?;
                let virtual_address = u64le(bytes, p + 16)?;
                let end = offset
                    .checked_add(file_size)
                    .ok_or(ElfError::InvalidHeader)?;
                if file_size % 16 != 0
                    || file_size as u64 > memory_size
                    || virtual_address.checked_add(memory_size).is_none()
                    || end > bytes.len()
                {
                    return Err(ElfError::InvalidHeader);
                }
                dynamic = Some((offset, file_size / 16, virtual_address, file_size));
                continue;
            }
            if segment_type != PT_LOAD {
                continue;
            }
            if count == N {
                return Err(ElfError::TooManySegments);
            }
            let flags = u32le(bytes, p + 4)?;
            let file_offset = u64le(bytes, p + 8)?;
            let virtual_address = u64le(bytes, p + 16)?
                .checked_add(load_bias)
                .ok_or(ElfError::InvalidSegment)?;
            let file_size = u64le(bytes, p + 32)?;
            let memory_size = u64le(bytes, p + 40)?;
            let alignment = u64le(bytes, p + 48)?;
            let file_end = file_offset
                .checked_add(file_size)
                .ok_or(ElfError::InvalidSegment)?;
            let mem_end = virtual_address
                .checked_add(memory_size)
                .ok_or(ElfError::InvalidSegment)?;
            if flags & !(PF_R | PF_W | PF_X) != 0
                || flags == 0
                || file_size > memory_size
                || file_end > bytes.len() as u64
                || virtual_address < 0x1000
                || mem_end > USER_TOP
                || memory_size == 0
                || (flags & (PF_W | PF_X)) == (PF_W | PF_X)
                || (alignment > 1
                    && (!alignment.is_power_of_two()
                        || file_offset % alignment != virtual_address % alignment))
            {
                return Err(ElfError::InvalidSegment);
            }
            for existing in &segments[..count] {
                let end = existing.virtual_address + existing.memory_size;
                if virtual_address < end && existing.virtual_address < mem_end {
                    return Err(ElfError::OverlappingSegments);
                }
            }
            if flags & PF_X != 0 && entry >= virtual_address && entry < mem_end {
                entry_executable = true;
            }
            segments[count] = LoadSegment {
                file_offset,
                virtual_address,
                file_size,
                memory_size,
                flags,
                alignment,
            };
            count += 1;
        }
        if count == 0 || !entry_executable {
            return Err(ElfError::EntryNotExecutable);
        }
        let (rela_file_offset, rela_count) = match dynamic {
            Some((offset, entries, virtual_address, file_size)) => {
                let dynamic_end = virtual_address
                    .checked_add(file_size as u64)
                    .ok_or(ElfError::InvalidHeader)?;
                let mapped = segments[..count].iter().any(|segment| {
                    let segment_va = segment.virtual_address - load_bias;
                    let segment_end = segment_va.checked_add(segment.file_size);
                    let offset_delta = virtual_address.checked_sub(segment_va);
                    segment_end.is_some_and(|end| dynamic_end <= end)
                        && offset_delta.and_then(|delta| segment.file_offset.checked_add(delta))
                            == Some(offset as u64)
                });
                if !mapped {
                    return Err(ElfError::InvalidHeader);
                }
                parse_relative_relocations(bytes, offset, entries, &segments[..count], load_bias)?
            }
            None => (0, 0),
        };
        let program_header_count = u64::try_from(phnum).map_err(|_| ElfError::InvalidHeader)?;
        let program_headers = match explicit_phdr {
            Some(address) => address,
            None => {
                let phoff_u64 = u64::try_from(phoff).map_err(|_| ElfError::InvalidHeader)?;
                let mut address = None;
                for segment in segments.iter().take(count).copied() {
                    let file_end = segment
                        .file_offset
                        .checked_add(segment.file_size)
                        .ok_or(ElfError::InvalidHeader)?;
                    let phbytes_u64 =
                        u64::try_from(phbytes).map_err(|_| ElfError::InvalidHeader)?;
                    let phend = phoff_u64
                        .checked_add(phbytes_u64)
                        .ok_or(ElfError::InvalidHeader)?;
                    if phoff_u64 >= segment.file_offset && phend <= file_end {
                        address = Some(
                            segment
                                .virtual_address
                                .checked_add(phoff_u64 - segment.file_offset)
                                .ok_or(ElfError::InvalidHeader)?,
                        );
                        break;
                    }
                }
                address.unwrap_or(0)
            }
        };
        Ok(Self {
            entry,
            segments,
            segment_count: count,
            load_bias,
            rela_file_offset,
            rela_count,
            program_headers,
            program_header_count,
            position_independent: kind == 3,
        })
    }

    fn is_position_independent(&self) -> bool {
        self.position_independent
    }

    fn load_into_unrelocated<T: ElfLoadTarget>(
        &self,
        bytes: &[u8],
        target: &mut T,
    ) -> Result<(), ElfError> {
        // `parse_for_dynamic_loader` guarantees the dynamic table is deferred;
        // an empty relocation list keeps all relocation ownership in ld-linux.
        self.load_into(bytes, target)
    }

    pub fn segments(&self) -> &[LoadSegment] {
        &self.segments[..self.segment_count]
    }

    fn mapped_range(&self) -> Result<(u64, u64), ElfError> {
        let mut start = u64::MAX;
        let mut end = 0u64;
        for segment in self.segments() {
            start = start.min(segment.virtual_address & !0xfff);
            let segment_end = segment
                .virtual_address
                .checked_add(segment.memory_size)
                .ok_or(ElfError::InvalidSegment)?;
            end = end.max(
                segment_end
                    .checked_add(0xfff)
                    .ok_or(ElfError::InvalidSegment)?
                    & !0xfff,
            );
        }
        if start == u64::MAX || end <= start {
            return Err(ElfError::InvalidSegment);
        }
        Ok((start, end - start))
    }

    /// Mapeia/copia segmentos PT_LOAD, zera BSS (as páginas começam zeradas)
    /// e só então aplica permissões finais. Em qualquer erro, desfaz todas as
    /// páginas criadas nesta chamada; falha no rollback é reportada ao caller
    /// pelo mesmo erro, que deve descartar o espaço de endereços inteiro.
    pub fn load_into<T: ElfLoadTarget>(
        &self,
        bytes: &[u8],
        target: &mut T,
    ) -> Result<(), ElfError> {
        let mut mapped = [0u64; MAX_LOAD_PAGES];
        let mut mapped_count = 0usize;
        let result = (|| {
            for segment in self.segments() {
                let start = segment.virtual_address & !(PAGE_SIZE - 1);
                let end = segment
                    .virtual_address
                    .checked_add(segment.memory_size)
                    .and_then(|v| v.checked_add(PAGE_SIZE - 1))
                    .ok_or(ElfError::InvalidSegment)?
                    & !(PAGE_SIZE - 1);
                let mut page = start;
                while page < end {
                    if mapped[..mapped_count].contains(&page) {
                        return Err(ElfError::OverlappingSegments);
                    }
                    if mapped_count == MAX_LOAD_PAGES {
                        return Err(ElfError::TooManySegments);
                    }
                    target
                        .map_zeroed_page(page)
                        .map_err(|_| ElfError::MappingFailed)?;
                    mapped[mapped_count] = page;
                    mapped_count += 1;
                    page = page
                        .checked_add(PAGE_SIZE)
                        .ok_or(ElfError::InvalidSegment)?;
                }
                let source_start =
                    usize::try_from(segment.file_offset).map_err(|_| ElfError::InvalidSegment)?;
                let source_end = source_start
                    .checked_add(
                        usize::try_from(segment.file_size).map_err(|_| ElfError::InvalidSegment)?,
                    )
                    .ok_or(ElfError::InvalidSegment)?;
                let source = bytes
                    .get(source_start..source_end)
                    .ok_or(ElfError::Truncated)?;
                target
                    .write(segment.virtual_address, source)
                    .map_err(|_| ElfError::MappingFailed)?;
            }
            for index in 0..self.rela_count {
                let offset = self.rela_file_offset + index * 24;
                let relocation = u64le(bytes, offset).map_err(|_| ElfError::InvalidHeader)?;
                let info = u64le(bytes, offset + 8).map_err(|_| ElfError::InvalidHeader)?;
                let target_end = relocation.checked_add(8).ok_or(ElfError::InvalidSegment)?;
                if info >> 32 != 0 || info as u32 != R_X86_64_RELATIVE {
                    return Err(ElfError::UnsupportedRelocation);
                }
                if !self.segments().iter().any(|segment| {
                    segment.flags & PF_W != 0
                        && relocation >= segment.virtual_address - self.load_bias
                        && target_end
                            <= segment.virtual_address - self.load_bias + segment.memory_size
                }) {
                    return Err(ElfError::InvalidSegment);
                }
                let addend = i64::from_le_bytes(
                    bytes
                        .get(offset + 16..offset + 24)
                        .ok_or(ElfError::InvalidHeader)?
                        .try_into()
                        .map_err(|_| ElfError::InvalidHeader)?,
                );
                let target_address = relocation
                    .checked_add(self.load_bias)
                    .ok_or(ElfError::InvalidSegment)?;
                let relocated = i128::from(self.load_bias) + i128::from(addend);
                let relocated = u64::try_from(relocated).map_err(|_| ElfError::InvalidSegment)?;
                target
                    .write(target_address, &relocated.to_le_bytes())
                    .map_err(|_| ElfError::MappingFailed)?;
            }
            for segment in self.segments() {
                let start = segment.virtual_address & !(PAGE_SIZE - 1);
                let end = segment
                    .virtual_address
                    .checked_add(segment.memory_size)
                    .and_then(|v| v.checked_add(PAGE_SIZE - 1))
                    .ok_or(ElfError::InvalidSegment)?
                    & !(PAGE_SIZE - 1);
                let mut page = start;
                while page < end {
                    target
                        .protect_page(page, segment.flags)
                        .map_err(|_| ElfError::MappingFailed)?;
                    page += PAGE_SIZE;
                }
            }
            Ok(())
        })();
        if result.is_err() {
            for page in mapped[..mapped_count].iter().rev() {
                let _ = target.unmap_page(*page);
            }
        }
        result
    }
}

fn parse_relative_relocations(
    bytes: &[u8],
    dynamic_offset: usize,
    dynamic_entries: usize,
    segments: &[LoadSegment],
    load_bias: u64,
) -> Result<(usize, usize), ElfError> {
    let mut rela_address = None;
    let mut rela_size = None;
    let mut rela_entry_size = None;
    let mut rela_count_tag = None;
    let mut terminated = false;
    for index in 0..dynamic_entries {
        let offset = dynamic_offset
            .checked_add(index.checked_mul(16).ok_or(ElfError::InvalidHeader)?)
            .ok_or(ElfError::InvalidHeader)?;
        let tag = i64::from_le_bytes(
            bytes
                .get(offset..offset + 8)
                .ok_or(ElfError::InvalidHeader)?
                .try_into()
                .map_err(|_| ElfError::InvalidHeader)?,
        );
        let value = u64le(bytes, offset + 8)?;
        if tag == DT_NULL {
            terminated = true;
            break;
        }
        match tag {
            DT_NEEDED | DT_PLTRELSZ | DT_REL | DT_RELSZ | DT_RELENT | DT_JMPREL => {
                return Err(ElfError::UnsupportedFormat)
            }
            DT_RELA if rela_address.replace(value).is_some() => {
                return Err(ElfError::InvalidHeader)
            }
            DT_RELASZ if rela_size.replace(value).is_some() => return Err(ElfError::InvalidHeader),
            DT_RELAENT if rela_entry_size.replace(value).is_some() => {
                return Err(ElfError::InvalidHeader)
            }
            DT_RELACOUNT if rela_count_tag.replace(value).is_some() => {
                return Err(ElfError::InvalidHeader)
            }
            DT_RELA | DT_RELASZ | DT_RELAENT | DT_RELACOUNT => {}
            _ => {}
        }
    }
    if !terminated {
        return Err(ElfError::InvalidHeader);
    }
    let (Some(rela_address), Some(rela_size), Some(24)) =
        (rela_address, rela_size, rela_entry_size)
    else {
        if rela_address.is_none() && rela_size.is_none() && rela_entry_size.is_none() {
            return Ok((0, 0));
        }
        return Err(ElfError::InvalidHeader);
    };
    if rela_size % 24 != 0 || rela_size / 24 > 2048 {
        return Err(ElfError::InvalidHeader);
    }
    let count = usize::try_from(rela_size / 24).map_err(|_| ElfError::InvalidHeader)?;
    if rela_count_tag.is_some_and(|tag_count| tag_count != count as u64) {
        return Err(ElfError::InvalidHeader);
    }
    let rela_end = rela_address
        .checked_add(rela_size)
        .ok_or(ElfError::InvalidHeader)?;
    let mut rela_file_offset = None;
    for segment in segments {
        let segment_end = segment
            .virtual_address
            .checked_sub(load_bias)
            .and_then(|start| start.checked_add(segment.file_size))
            .ok_or(ElfError::InvalidSegment)?;
        if rela_address >= segment.virtual_address - load_bias && rela_end <= segment_end {
            let segment_va = segment.virtual_address - load_bias;
            let delta = rela_address - segment_va;
            rela_file_offset = Some(
                usize::try_from(
                    segment
                        .file_offset
                        .checked_add(delta)
                        .ok_or(ElfError::InvalidHeader)?,
                )
                .map_err(|_| ElfError::InvalidHeader)?,
            );
            break;
        }
    }
    let file_offset = rela_file_offset.ok_or(ElfError::InvalidHeader)?;
    let file_end = file_offset
        .checked_add(usize::try_from(rela_size).map_err(|_| ElfError::InvalidHeader)?)
        .ok_or(ElfError::InvalidHeader)?;
    if file_end > bytes.len() {
        return Err(ElfError::Truncated);
    }
    for index in 0..count {
        let offset = file_offset + index * 24;
        let target = u64le(bytes, offset)?;
        let info = u64le(bytes, offset + 8)?;
        let addend = i64::from_le_bytes(
            bytes[offset + 16..offset + 24]
                .try_into()
                .map_err(|_| ElfError::InvalidHeader)?,
        );
        if info >> 32 != 0 || info as u32 != R_X86_64_RELATIVE {
            return Err(ElfError::UnsupportedRelocation);
        }
        let target_end = target.checked_add(8).ok_or(ElfError::InvalidSegment)?;
        if !segments.iter().any(|segment| {
            segment.flags & PF_W != 0
                && target >= segment.virtual_address - load_bias
                && target_end <= segment.virtual_address - load_bias + segment.memory_size
        }) {
            return Err(ElfError::InvalidSegment);
        }
        let relocated = i128::from(load_bias) + i128::from(addend);
        u64::try_from(relocated).map_err(|_| ElfError::InvalidSegment)?;
    }
    Ok((file_offset, count))
}

fn u16le(b: &[u8], o: usize) -> Result<u16, ElfError> {
    Ok(u16::from_le_bytes(
        b.get(o..o + 2)
            .ok_or(ElfError::Truncated)?
            .try_into()
            .unwrap(),
    ))
}
fn u32le(b: &[u8], o: usize) -> Result<u32, ElfError> {
    Ok(u32::from_le_bytes(
        b.get(o..o + 4)
            .ok_or(ElfError::Truncated)?
            .try_into()
            .unwrap(),
    ))
}
fn u64le(b: &[u8], o: usize) -> Result<u64, ElfError> {
    Ok(u64::from_le_bytes(
        b.get(o..o + 8)
            .ok_or(ElfError::Truncated)?
            .try_into()
            .unwrap(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Target {
        pages: [u64; 4],
        count: usize,
        flags: u32,
    }
    struct StackTarget {
        base: u64,
        bytes: [u8; 4096],
    }
    struct RelocationTarget {
        relocation: Option<(u64, [u8; 8])>,
    }
    impl ElfLoadTarget for StackTarget {
        type Error = ();
        fn map_zeroed_page(&mut self, _page: u64) -> Result<(), Self::Error> {
            Ok(())
        }
        fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), Self::Error> {
            let start =
                usize::try_from(address.checked_sub(self.base).ok_or(())?).map_err(|_| ())?;
            let end = start.checked_add(bytes.len()).ok_or(())?;
            self.bytes
                .get_mut(start..end)
                .ok_or(())?
                .copy_from_slice(bytes);
            Ok(())
        }
        fn protect_page(&mut self, _page: u64, _flags: u32) -> Result<(), Self::Error> {
            Ok(())
        }
        fn unmap_page(&mut self, _page: u64) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    impl StackTarget {
        fn word(&self, address: u64) -> u64 {
            let offset = (address - self.base) as usize;
            u64::from_le_bytes(self.bytes[offset..offset + 8].try_into().unwrap())
        }
        fn c_string(&self, address: u64) -> &[u8] {
            let offset = (address - self.base) as usize;
            let tail = &self.bytes[offset..];
            &tail[..tail.iter().position(|byte| *byte == 0).unwrap()]
        }
    }
    impl ElfLoadTarget for Target {
        type Error = ();
        fn map_zeroed_page(&mut self, page: u64) -> Result<(), Self::Error> {
            self.pages[self.count] = page;
            self.count += 1;
            Ok(())
        }
        fn write(&mut self, _address: u64, _bytes: &[u8]) -> Result<(), Self::Error> {
            Ok(())
        }
        fn protect_page(&mut self, _page: u64, flags: u32) -> Result<(), Self::Error> {
            self.flags = flags;
            Ok(())
        }
        fn unmap_page(&mut self, _page: u64) -> Result<(), Self::Error> {
            Ok(())
        }
    }
    impl ElfLoadTarget for RelocationTarget {
        type Error = ();
        fn map_zeroed_page(&mut self, _page: u64) -> Result<(), Self::Error> {
            Ok(())
        }
        fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), Self::Error> {
            if address == 0x601080 && bytes.len() == 8 {
                self.relocation = Some((address, bytes.try_into().map_err(|_| ())?));
            }
            Ok(())
        }
        fn protect_page(&mut self, _page: u64, _flags: u32) -> Result<(), Self::Error> {
            Ok(())
        }
        fn unmap_page(&mut self, _page: u64) -> Result<(), Self::Error> {
            Ok(())
        }
    }
    fn fixture() -> [u8; 120] {
        let mut b = [0u8; 120];
        b[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        b[16..18].copy_from_slice(&2u16.to_le_bytes());
        b[18..20].copy_from_slice(&62u16.to_le_bytes());
        b[20..24].copy_from_slice(&1u32.to_le_bytes());
        b[24..32].copy_from_slice(&0x400000u64.to_le_bytes());
        b[32..40].copy_from_slice(&64u64.to_le_bytes());
        b[52..54].copy_from_slice(&64u16.to_le_bytes());
        b[54..56].copy_from_slice(&56u16.to_le_bytes());
        b[56..58].copy_from_slice(&1u16.to_le_bytes());
        b[64..68].copy_from_slice(&PT_LOAD.to_le_bytes());
        b[68..72].copy_from_slice(&5u32.to_le_bytes());
        b[72..80].copy_from_slice(&120u64.to_le_bytes());
        b[80..88].copy_from_slice(&0x400000u64.to_le_bytes());
        b[96..104].copy_from_slice(&0u64.to_le_bytes());
        b[104..112].copy_from_slice(&16u64.to_le_bytes());
        b
    }

    #[test]
    fn dynamic_metadata_reads_pt_interp_and_rejects_unsafe_paths() {
        let mut bytes = vec![0u8; 192];
        bytes[..120].copy_from_slice(&fixture());
        bytes[56..58].copy_from_slice(&2u16.to_le_bytes());
        let ph = 64 + PROGRAM_HEADER_SIZE;
        bytes[ph..ph + 4].copy_from_slice(&PT_INTERP.to_le_bytes());
        bytes[ph + 8..ph + 16].copy_from_slice(&176u64.to_le_bytes());
        bytes[ph + 32..ph + 40].copy_from_slice(&8u64.to_le_bytes());
        bytes[176..184].copy_from_slice(b"/lib/ld\0");
        let metadata = ElfDynamicMetadata::<4>::parse(&bytes).unwrap();
        assert_eq!(metadata.interpreter(), Some("/lib/ld"));
        assert_eq!(metadata.dependencies().count(), 0);

        bytes[176..185].copy_from_slice(b"../ld.so\0");
        bytes[ph + 32..ph + 40].copy_from_slice(&9u64.to_le_bytes());
        assert_eq!(
            ElfDynamicMetadata::<4>::parse(&bytes),
            Err(ElfError::DynamicMetadataInvalid)
        );
    }

    #[test]
    fn vfs_exec_loads_pt_interp_and_builds_linux_auxv() {
        use crate::vfs::{AccessContext, Credentials, Vfs};

        let root = Credentials { uid: 0, gid: 0 };
        let access = AccessContext::new(root, &[], 0);
        let mut vfs = Vfs::<8>::new();
        vfs.create_dir("/", "bin", root, 0o755).unwrap();
        vfs.create_dir("/", "lib", root, 0o755).unwrap();
        vfs.create_file("/bin", "demo", root, 0o755).unwrap();
        vfs.create_file("/lib", "ld.so", root, 0o755).unwrap();

        let mut program = vec![0u8; 256];
        program[..120].copy_from_slice(&fixture());
        program[16..18].copy_from_slice(&2u16.to_le_bytes());
        program[56..58].copy_from_slice(&2u16.to_le_bytes());
        program[72..80].copy_from_slice(&0u64.to_le_bytes());
        program[80..88].copy_from_slice(&0x400000u64.to_le_bytes());
        program[96..104].copy_from_slice(&256u64.to_le_bytes());
        program[104..112].copy_from_slice(&4096u64.to_le_bytes());
        let interp_ph = 64 + PROGRAM_HEADER_SIZE;
        program[interp_ph..interp_ph + 4].copy_from_slice(&PT_INTERP.to_le_bytes());
        program[interp_ph + 8..interp_ph + 16].copy_from_slice(&240u64.to_le_bytes());
        program[interp_ph + 32..interp_ph + 40].copy_from_slice(&11u64.to_le_bytes());
        program[240..251].copy_from_slice(b"/lib/ld.so\0");

        let mut interpreter = vec![0u8; 176];
        interpreter[..120].copy_from_slice(&fixture());
        interpreter[16..18].copy_from_slice(&3u16.to_le_bytes());
        interpreter[24..32].copy_from_slice(&0x100u64.to_le_bytes());
        interpreter[56..58].copy_from_slice(&1u16.to_le_bytes());
        interpreter[72..80].copy_from_slice(&0u64.to_le_bytes());
        interpreter[80..88].copy_from_slice(&0u64.to_le_bytes());
        interpreter[96..104].copy_from_slice(&176u64.to_le_bytes());
        interpreter[104..112].copy_from_slice(&4096u64.to_le_bytes());

        vfs.write_file_as("/bin/demo", access, &program).unwrap();
        vfs.write_file_as("/lib/ld.so", access, &interpreter)
            .unwrap();
        let mut program_scratch = [0; 512];
        let mut interpreter_scratch = [0; 512];
        let mut target = Target {
            pages: [0; 4],
            count: 0,
            flags: 0,
        };
        let image = load_interpreted_executable_from_vfs::<4, 8, _>(
            &vfs,
            "/bin/demo",
            access,
            &mut program_scratch,
            &mut interpreter_scratch,
            &mut target,
        )
        .unwrap();
        assert_eq!(image.entry, DYNAMIC_INTERPRETER_BIAS + 0x100);
        assert_eq!(image.program_entry, 0x400000);
        assert_eq!(image.interpreter_base, DYNAMIC_INTERPRETER_BIAS);
        assert_eq!(image.program_headers, 0x400000 + 64);
        assert_eq!(image.program_header_count, 2);
        assert_eq!(target.count, 2);
        let auxv = image.auxv();
        assert_eq!(
            auxv[0],
            AuxEntry {
                kind: AT_PHDR,
                value: 0x400000 + 64
            }
        );
        assert_eq!(
            auxv[4],
            AuxEntry {
                kind: AT_BASE,
                value: DYNAMIC_INTERPRETER_BIAS
            }
        );
        assert_eq!(
            auxv[5],
            AuxEntry {
                kind: AT_ENTRY,
                value: 0x400000
            }
        );
    }

    #[test]
    fn dynamic_metadata_reads_needed_names_from_mapped_string_table() {
        let mut bytes = vec![0u8; 512];
        bytes[..120].copy_from_slice(&fixture());
        bytes[56..58].copy_from_slice(&3u16.to_le_bytes());
        let load = 64 + PROGRAM_HEADER_SIZE;
        bytes[load..load + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
        bytes[load + 8..load + 16].copy_from_slice(&256u64.to_le_bytes());
        bytes[load + 16..load + 24].copy_from_slice(&0x500000u64.to_le_bytes());
        bytes[load + 32..load + 40].copy_from_slice(&128u64.to_le_bytes());
        bytes[load + 40..load + 48].copy_from_slice(&128u64.to_le_bytes());
        let dynamic = load + PROGRAM_HEADER_SIZE;
        bytes[dynamic..dynamic + 4].copy_from_slice(&PT_DYNAMIC.to_le_bytes());
        bytes[dynamic + 8..dynamic + 16].copy_from_slice(&256u64.to_le_bytes());
        bytes[dynamic + 16..dynamic + 24].copy_from_slice(&0x500000u64.to_le_bytes());
        bytes[dynamic + 32..dynamic + 40].copy_from_slice(&80u64.to_le_bytes());
        bytes[dynamic + 40..dynamic + 48].copy_from_slice(&80u64.to_le_bytes());
        let mut cursor = 256;
        for (tag, value) in [
            (DT_STRTAB, 0x500060u64),
            (DT_STRSZ, 21),
            (DT_NEEDED, 1),
            (DT_NEEDED, 11),
            (DT_NULL, 0),
        ] {
            bytes[cursor..cursor + 8].copy_from_slice(&tag.to_le_bytes());
            bytes[cursor + 8..cursor + 16].copy_from_slice(&value.to_le_bytes());
            cursor += 16;
        }
        bytes[352..373].copy_from_slice(b"\0libc.so.6\0libm.so.6\0");
        let metadata = ElfDynamicMetadata::<4>::parse(&bytes).unwrap();
        assert_eq!(
            metadata.dependencies().collect::<std::vec::Vec<_>>(),
            ["libc.so.6", "libm.so.6"]
        );
    }

    #[test]
    fn dynamic_metadata_rejects_u64_needed_offset_overflow() {
        let mut bytes = vec![0u8; 512];
        bytes[..120].copy_from_slice(&fixture());
        bytes[56..58].copy_from_slice(&3u16.to_le_bytes());
        let load = 64 + PROGRAM_HEADER_SIZE;
        bytes[load..load + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
        bytes[load + 8..load + 16].copy_from_slice(&256u64.to_le_bytes());
        bytes[load + 16..load + 24].copy_from_slice(&0x500000u64.to_le_bytes());
        bytes[load + 32..load + 40].copy_from_slice(&128u64.to_le_bytes());
        bytes[load + 40..load + 48].copy_from_slice(&128u64.to_le_bytes());
        let dynamic = load + PROGRAM_HEADER_SIZE;
        bytes[dynamic..dynamic + 4].copy_from_slice(&PT_DYNAMIC.to_le_bytes());
        bytes[dynamic + 8..dynamic + 16].copy_from_slice(&256u64.to_le_bytes());
        bytes[dynamic + 16..dynamic + 24].copy_from_slice(&0x500000u64.to_le_bytes());
        bytes[dynamic + 32..dynamic + 40].copy_from_slice(&80u64.to_le_bytes());
        bytes[dynamic + 40..dynamic + 48].copy_from_slice(&80u64.to_le_bytes());
        for (index, (tag, value)) in [
            (DT_STRTAB, 0x500060u64),
            (DT_STRSZ, 21),
            (DT_NEEDED, u64::from(u32::MAX) + 1),
            (DT_NULL, 0),
        ]
        .into_iter()
        .enumerate()
        {
            let at = 256 + index * 16;
            bytes[at..at + 8].copy_from_slice(&tag.to_le_bytes());
            bytes[at + 8..at + 16].copy_from_slice(&value.to_le_bytes());
        }
        bytes[352..373].copy_from_slice(b"\0libc.so.6\0libm.so.6\0");

        assert_eq!(
            ElfDynamicMetadata::<4>::parse(&bytes),
            Err(ElfError::DynamicMetadataInvalid)
        );
    }

    fn relative_relocation_pie() -> Vec<u8> {
        let mut b = vec![0u8; 0x2060];
        b[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        b[16..18].copy_from_slice(&3u16.to_le_bytes());
        b[18..20].copy_from_slice(&62u16.to_le_bytes());
        b[20..24].copy_from_slice(&1u32.to_le_bytes());
        b[24..32].copy_from_slice(&0x400000u64.to_le_bytes());
        b[32..40].copy_from_slice(&64u64.to_le_bytes());
        b[52..54].copy_from_slice(&64u16.to_le_bytes());
        b[54..56].copy_from_slice(&56u16.to_le_bytes());
        b[56..58].copy_from_slice(&3u16.to_le_bytes());

        let first = 64;
        b[first..first + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
        b[first + 4..first + 8].copy_from_slice(&(PF_R | PF_X).to_le_bytes());
        b[first + 8..first + 16].copy_from_slice(&0x1000u64.to_le_bytes());
        b[first + 16..first + 24].copy_from_slice(&0x400000u64.to_le_bytes());
        b[first + 32..first + 40].copy_from_slice(&1u64.to_le_bytes());
        b[first + 40..first + 48].copy_from_slice(&1u64.to_le_bytes());
        b[first + 48..first + 56].copy_from_slice(&0x1000u64.to_le_bytes());
        b[0x1000] = 0xc3;

        let second = first + PROGRAM_HEADER_SIZE;
        b[second..second + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
        b[second + 4..second + 8].copy_from_slice(&(PF_R | PF_W).to_le_bytes());
        b[second + 8..second + 16].copy_from_slice(&0x2000u64.to_le_bytes());
        b[second + 16..second + 24].copy_from_slice(&0x401000u64.to_le_bytes());
        b[second + 32..second + 40].copy_from_slice(&0x60u64.to_le_bytes());
        b[second + 40..second + 48].copy_from_slice(&0x100u64.to_le_bytes());
        b[second + 48..second + 56].copy_from_slice(&0x1000u64.to_le_bytes());

        let dynamic = second + PROGRAM_HEADER_SIZE;
        b[dynamic..dynamic + 4].copy_from_slice(&PT_DYNAMIC.to_le_bytes());
        b[dynamic + 8..dynamic + 16].copy_from_slice(&0x2000u64.to_le_bytes());
        b[dynamic + 16..dynamic + 24].copy_from_slice(&0x401000u64.to_le_bytes());
        b[dynamic + 32..dynamic + 40].copy_from_slice(&64u64.to_le_bytes());
        b[dynamic + 40..dynamic + 48].copy_from_slice(&64u64.to_le_bytes());
        for (index, (tag, value)) in [
            (DT_RELA, 0x401040u64),
            (DT_RELASZ, 24),
            (DT_RELAENT, 24),
            (DT_NULL, 0),
        ]
        .into_iter()
        .enumerate()
        {
            let offset = 0x2000 + index * 16;
            b[offset..offset + 8].copy_from_slice(&tag.to_le_bytes());
            b[offset + 8..offset + 16].copy_from_slice(&value.to_le_bytes());
        }
        b[0x2040..0x2048].copy_from_slice(&0x401080u64.to_le_bytes());
        b[0x2048..0x2050].copy_from_slice(&(R_X86_64_RELATIVE as u64).to_le_bytes());
        b[0x2050..0x2058].copy_from_slice(&0x123u64.to_le_bytes());
        b
    }
    #[test]
    fn parses_executable_load_segment() {
        let e = ElfImage::<4>::parse(&fixture()).unwrap();
        assert_eq!(e.entry, 0x400000);
        assert_eq!(e.segments().len(), 1);
    }

    #[test]
    fn loads_elf_from_vfs_after_execute_and_read_checks() {
        use crate::vfs::{AccessContext, Credentials, Vfs};

        let root = Credentials { uid: 0, gid: 0 };
        let access = AccessContext::new(root, &[], 0);
        let mut vfs = Vfs::<8>::new();
        vfs.create_dir("/", "programas", root, 0o755).unwrap();
        vfs.create_file("/programas", "app", root, 0o755).unwrap();
        vfs.write_file_as("/programas/app", access, &fixture())
            .unwrap();

        let mut scratch = [0; 256];
        let mut target = Target {
            pages: [0; 4],
            count: 0,
            flags: 0,
        };
        let (image, length) = load_executable_from_vfs::<4, 8, _>(
            &vfs,
            "/programas/app",
            access,
            &mut scratch,
            &mut target,
        )
        .unwrap();
        assert_eq!(image.entry, 0x400000);
        assert_eq!(length, 120);
        assert_eq!(target.count, 1);
    }

    #[test]
    fn vfs_elf_loader_rejects_non_executable_files_before_loading() {
        use crate::vfs::{AccessContext, Credentials, Vfs};

        let root = Credentials { uid: 0, gid: 0 };
        let access = AccessContext::new(root, &[], 0);
        let mut vfs = Vfs::<8>::new();
        vfs.create_dir("/", "programas", root, 0o755).unwrap();
        vfs.create_file("/programas", "app", root, 0o644).unwrap();
        vfs.write_file_as("/programas/app", access, &fixture())
            .unwrap();

        let mut scratch = [0; 256];
        let mut target = Target {
            pages: [0; 4],
            count: 0,
            flags: 0,
        };
        assert_eq!(
            load_executable_from_vfs::<4, 8, _>(
                &vfs,
                "/programas/app",
                access,
                &mut scratch,
                &mut target,
            ),
            Err(VfsElfError::Vfs(crate::vfs::VfsError::PermissionDenied))
        );
        assert_eq!(target.count, 0);
    }
    #[test]
    fn maps_zeroed_segment_and_applies_final_permissions() {
        let bytes = fixture();
        let image = ElfImage::<4>::parse(&bytes).unwrap();
        let mut target = Target {
            pages: [0; 4],
            count: 0,
            flags: 0,
        };
        image.load_into(&bytes, &mut target).unwrap();
        assert_eq!(target.count, 1);
        assert_eq!(target.pages[0], 0x400000);
        assert_eq!(target.flags, PF_R | PF_X);
    }

    #[test]
    fn builds_initial_stack_with_argc_argv_envp_and_terminators() {
        let mut target = StackTarget {
            base: 0x4000_0000,
            bytes: [0; 4096],
        };
        let stack = build_initial_user_stack(
            &mut target,
            0x4000_1000,
            &["/bin/demo", "--safe"],
            &["PATH=/bin", "MODE=test"],
        )
        .unwrap();
        assert_eq!(stack.stack_pointer & 0xf, 0);
        assert_eq!(stack.argc, 2);
        assert_eq!(target.word(stack.stack_pointer), 2);
        let arg0 = target.word(stack.argv);
        let arg1 = target.word(stack.argv + 8);
        assert_eq!(target.word(stack.argv + 16), 0);
        assert_eq!(target.c_string(arg0), b"/bin/demo");
        assert_eq!(target.c_string(arg1), b"--safe");
        let env0 = target.word(stack.envp);
        let env1 = target.word(stack.envp + 8);
        assert_eq!(target.word(stack.envp + 16), 0);
        assert_eq!(target.c_string(env0), b"PATH=/bin");
        assert_eq!(target.c_string(env1), b"MODE=test");
    }

    #[test]
    fn builds_initial_stack_with_terminated_auxv_pairs() {
        let mut target = StackTarget {
            base: 0x4000_0000,
            bytes: [0; 4096],
        };
        let auxv = [
            AuxEntry {
                kind: AT_PHDR,
                value: 0x400040,
            },
            AuxEntry {
                kind: AT_ENTRY,
                value: 0x401000,
            },
        ];
        let stack = build_initial_user_stack_with_auxv(
            &mut target,
            0x4000_1000,
            &["/bin/demo"],
            &["MODE=test"],
            &auxv,
        )
        .unwrap();
        let auxv_start = stack.envp + 16;
        assert_eq!(target.word(auxv_start), AT_PHDR);
        assert_eq!(target.word(auxv_start + 8), 0x400040);
        assert_eq!(target.word(auxv_start + 16), AT_ENTRY);
        assert_eq!(target.word(auxv_start + 24), 0x401000);
        assert_eq!(target.word(auxv_start + 32), AT_NULL);
        assert_eq!(target.word(auxv_start + 40), 0);
    }

    #[test]
    fn rejects_initial_stack_overflow_and_embedded_nul() {
        let mut target = Target {
            pages: [0; 4],
            count: 0,
            flags: 0,
        };
        assert_eq!(
            build_initial_user_stack(&mut target, 0x4000_1000, &["bad\0arg"], &[]),
            Err(ElfError::InvalidArguments)
        );
        assert_eq!(
            build_initial_user_stack(&mut target, 0x4000_1000, &["x"; 17], &["y"; 17]),
            Err(ElfError::InvalidArguments)
        );
    }
    #[test]
    fn rejects_writable_executable_segment() {
        let mut b = fixture();
        b[68..72].copy_from_slice(&7u32.to_le_bytes());
        assert_eq!(ElfImage::<4>::parse(&b), Err(ElfError::InvalidSegment));
    }
    #[test]
    fn rejects_truncated_file_payload() {
        let b = fixture();
        assert_eq!(ElfImage::<4>::parse(&b[..119]), Err(ElfError::Truncated));
    }

    #[test]
    fn loads_static_pie_et_dyn_with_checked_load_bias() {
        let mut b = fixture();
        b[16..18].copy_from_slice(&3u16.to_le_bytes());
        b[24..32].copy_from_slice(&0u64.to_le_bytes());
        b[80..88].copy_from_slice(&0u64.to_le_bytes());
        let image = ElfImage::<4>::parse_with_load_bias(&b, 0x20_0000).unwrap();
        assert_eq!(image.entry, 0x20_0000);
        assert_eq!(image.segments()[0].virtual_address, 0x20_0000);
        assert_eq!(
            ElfImage::<4>::parse_with_load_bias(&b, USER_TOP),
            Err(ElfError::InvalidSegment)
        );
        assert_eq!(
            ElfImage::<4>::parse_with_load_bias(&b, 0x20_0001),
            Err(ElfError::UnsupportedFormat)
        );
    }

    #[test]
    fn applies_local_relative_relocations_in_static_pie() {
        let bytes = relative_relocation_pie();
        let image = ElfImage::<4>::parse_with_load_bias(&bytes, 0x200000).unwrap();
        assert_eq!(image.entry, 0x600000);
        let mut target = RelocationTarget { relocation: None };
        image.load_into(&bytes, &mut target).unwrap();
        assert_eq!(
            target.relocation,
            Some((0x601080, 0x200123u64.to_le_bytes()))
        );

        let mut symbol_relocation = bytes;
        symbol_relocation[0x204c..0x2050]
            .copy_from_slice(&((1u64 << 32) | R_X86_64_RELATIVE as u64).to_le_bytes()[4..]);
        assert_eq!(
            ElfImage::<4>::parse_with_load_bias(&symbol_relocation, 0x200000),
            Err(ElfError::UnsupportedRelocation)
        );
    }

    #[test]
    fn rejects_interpreters_and_malformed_dynamic_segments() {
        let base = fixture();
        let mut b = [0u8; ELF_HEADER_SIZE + 2 * PROGRAM_HEADER_SIZE + 16];
        b[..base.len()].copy_from_slice(&base);
        b[16..18].copy_from_slice(&3u16.to_le_bytes());
        b[24..32].copy_from_slice(&0u64.to_le_bytes());
        b[80..88].copy_from_slice(&0u64.to_le_bytes());
        b[56..58].copy_from_slice(&2u16.to_le_bytes());
        b[120..176].copy_from_slice(&base[64..120]);
        b[120..124].copy_from_slice(&PT_DYNAMIC.to_le_bytes());
        assert_eq!(
            ElfImage::<4>::parse_with_load_bias(&b, 0x20_0000),
            Err(ElfError::InvalidHeader)
        );
        b[120..124].copy_from_slice(&PT_INTERP.to_le_bytes());
        assert_eq!(
            ElfImage::<4>::parse_with_load_bias(&b, 0x20_0000),
            Err(ElfError::UnsupportedFormat)
        );
    }
}

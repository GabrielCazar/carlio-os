//! Carregador de módulos ELF64 ET_REL para x86_64.
//!
//! O parser e aplicador de relocations são independentes do backend. O backend
//! arquitetural deve alocar páginas RW/NX, publicar permissões finais sem W+X,
//! invocar init/exit e liberar as páginas após quiescência das CPUs.

const ELF_HEADER_SIZE: usize = 64;
const SECTION_HEADER_SIZE: usize = 64;
const SYMBOL_SIZE: usize = 24;
const RELA_SIZE: usize = 24;
const SHT_SYMTAB: u32 = 2;
const SHT_STRTAB: u32 = 3;
const SHT_RELA: u32 = 4;
const SHF_ALLOC: u64 = 2;
const SHF_WRITE: u64 = 1;
const SHF_EXECINSTR: u64 = 4;
const SHN_UNDEF: u16 = 0;
const SHN_ABS: u16 = 0xfff1;
const STB_LOCAL: u8 = 0;
const STB_GLOBAL: u8 = 1;
const STB_WEAK: u8 = 2;
const MAX_MODULE_EXPORTS: usize = 128;
const MODULE_SYMBOL_NAME_MAX: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleError {
    Truncated,
    InvalidElf,
    Unsupported,
    Bounds,
    WritableExecutable,
    Allocation,
    MissingSymbol,
    InvalidExportTable,
    Relocation,
    Protection,
    InitFailed,
    Busy,
    UnknownModule,
    Dependency,
    Capacity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SectionPermissions {
    pub writable: bool,
    pub executable: bool,
}

/// Símbolo público do kernel. A lista entregue ao loader deve estar ordenada
/// por nome, ser livre de duplicatas e conter endereços não nulos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KernelExport {
    pub name: &'static str,
    pub address: u64,
}

/// Índice kallsyms compacto e imutável, adequado para construir em boot a
/// partir da allowlist de ABI do kernel. Não exporta símbolos internos por
/// padrão.
#[derive(Clone, Copy, Debug)]
pub struct KernelSymbolTable<'a> {
    exports: &'a [KernelExport],
}

impl<'a> KernelSymbolTable<'a> {
    pub fn new(exports: &'a [KernelExport]) -> Result<Self, ModuleError> {
        for (index, entry) in exports.iter().enumerate() {
            if entry.name.is_empty() || entry.address == 0 {
                return Err(ModuleError::InvalidExportTable);
            }
            if index > 0 && exports[index - 1].name >= entry.name {
                return Err(ModuleError::InvalidExportTable);
            }
        }
        Ok(Self { exports })
    }

    pub fn resolve(&self, name: &str) -> Option<u64> {
        self.exports
            .binary_search_by(|entry| entry.name.cmp(name))
            .ok()
            .map(|index| self.exports[index].address)
    }
}

/// Export publicado por um módulo já carregado. A ordem do slice representa a
/// ordem de publicação; símbolos do kernel têm precedência.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModuleExport {
    pub module: &'static str,
    pub name: &'static str,
    pub address: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct ModuleSymbolTable<'a> {
    exports: &'a [ModuleExport],
}

impl<'a> ModuleSymbolTable<'a> {
    pub fn new(exports: &'a [ModuleExport]) -> Result<Self, ModuleError> {
        for (index, entry) in exports.iter().enumerate() {
            if entry.module.is_empty() || entry.name.is_empty() || entry.address == 0 {
                return Err(ModuleError::InvalidExportTable);
            }
            if exports[..index]
                .iter()
                .any(|previous| previous.module == entry.module && previous.name == entry.name)
            {
                return Err(ModuleError::InvalidExportTable);
            }
        }
        Ok(Self { exports })
    }

    pub fn resolve(&self, name: &str) -> Option<u64> {
        self.exports
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| entry.address)
    }
}

/// Resolver usado pelo backend durante relocations: procura primeiro na ABI
/// exportada pelo kernel e depois nos módulos previamente publicados.
#[derive(Clone, Copy, Debug)]
pub struct KernelDynamicLinker<'a> {
    pub kernel: KernelSymbolTable<'a>,
    pub modules: ModuleSymbolTable<'a>,
}

impl KernelDynamicLinker<'_> {
    pub fn resolve(&self, name: &str) -> Option<u64> {
        self.kernel
            .resolve(name)
            .or_else(|| self.modules.resolve(name))
    }
}

/// Todas as operações de memória/execução privilegiadas pertencem ao backend
/// x86_64. `allocate_rw_nx` jamais pode retornar uma região executável.
pub trait ModulePlatform {
    fn allocate_rw_nx(&mut self, size: usize, align: usize) -> Result<u64, ModuleError>;
    fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), ModuleError>;
    fn zero(&mut self, address: u64, size: usize) -> Result<(), ModuleError>;
    fn read_u64(&mut self, address: u64) -> Result<u64, ModuleError>;
    fn write_u64(&mut self, address: u64, value: u64) -> Result<(), ModuleError>;
    fn write_u32(&mut self, address: u64, value: u32) -> Result<(), ModuleError>;
    fn protect(
        &mut self,
        address: u64,
        size: usize,
        permissions: SectionPermissions,
    ) -> Result<(), ModuleError>;
    fn resolve_symbol(&self, name: &str) -> Option<u64>;
    fn resolve_symbol_with_provider(&self, name: &str) -> Option<(u64, Option<u32>)> {
        self.resolve_symbol(name).map(|address| (address, None))
    }
    fn invoke_init(&mut self, address: u64) -> Result<i32, ModuleError>;
    fn invoke_exit(&mut self, address: u64) -> Result<(), ModuleError>;
    /// Aguarda CPUs que possam executar código do módulo e completa shootdown.
    fn quiesce(&mut self, address: u64, size: usize) -> Result<(), ModuleError>;
    fn free(&mut self, address: u64, size: usize);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Section {
    kind: u32,
    flags: u64,
    offset: usize,
    size: usize,
    link: usize,
    info: usize,
    align: usize,
    entsize: usize,
}

#[derive(Clone, Copy, Debug)]
struct Allocation {
    address: u64,
    size: usize,
}

/// Carrega um objeto relocatable. Falhas anteriores ao init fazem rollback
/// completo; init != 0 também chama exit somente se init já retornou sucesso.
pub fn load<P: ModulePlatform>(
    platform: &mut P,
    image: &[u8],
) -> Result<LoadedModule, ModuleError> {
    load_inner(platform, image)
}

#[derive(Debug)]
pub struct LoadedModule {
    pub init_address: Option<u64>,
    pub exit_address: Option<u64>,
    allocations: [Option<Allocation>; 64],
    allocation_count: usize,
    exports: [Option<OwnedModuleExport>; MAX_MODULE_EXPORTS],
    export_count: usize,
    dependencies: [u32; 64],
    dependency_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OwnedModuleExport {
    name: [u8; MODULE_SYMBOL_NAME_MAX],
    name_len: u8,
    address: u64,
}

impl OwnedModuleExport {
    fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..usize::from(self.name_len)]).unwrap_or("")
    }
}

/// Registro persistente: exports permanecem associados às páginas do módulo
/// e provedores ocupados não podem ser removidos.
pub struct ModuleRegistry<const MODULES: usize> {
    entries: [Option<RegistryEntry>; MODULES],
    next_id: u32,
}

struct RegistryEntry {
    id: u32,
    name: [u8; MODULE_SYMBOL_NAME_MAX],
    name_len: u8,
    module: LoadedModule,
    dependents: u32,
}

impl<const MODULES: usize> ModuleRegistry<MODULES> {
    pub const fn new() -> Self {
        Self {
            entries: [const { None }; MODULES],
            next_id: 1,
        }
    }

    pub fn publish(&mut self, name: &str, mut module: LoadedModule) -> Result<u32, ModuleError> {
        if name.is_empty() || name.len() > MODULE_SYMBOL_NAME_MAX || !name.is_ascii() {
            return Err(ModuleError::InvalidElf);
        }
        if self
            .entries
            .iter()
            .flatten()
            .any(|entry| entry.module_name() == name)
        {
            return Err(ModuleError::InvalidExportTable);
        }
        let slot = self
            .entries
            .iter()
            .position(Option::is_none)
            .ok_or(ModuleError::Capacity)?;
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).ok_or(ModuleError::Capacity)?;
        for provider in module.dependencies[..module.dependency_count]
            .iter()
            .copied()
        {
            let entry = self
                .entries
                .iter_mut()
                .flatten()
                .find(|entry| entry.id == provider)
                .ok_or(ModuleError::Dependency)?;
            entry.dependents = entry
                .dependents
                .checked_add(1)
                .ok_or(ModuleError::Capacity)?;
        }
        let mut stored_name = [0; MODULE_SYMBOL_NAME_MAX];
        stored_name[..name.len()].copy_from_slice(name.as_bytes());
        module.dependencies[..module.dependency_count].sort_unstable();
        self.entries[slot] = Some(RegistryEntry {
            id,
            name: stored_name,
            name_len: name.len().try_into().map_err(|_| ModuleError::Capacity)?,
            module,
            dependents: 0,
        });
        Ok(id)
    }

    pub fn resolve(&self, name: &str) -> Option<(u64, u32)> {
        self.entries.iter().flatten().find_map(|entry| {
            entry.module.exports[..entry.module.export_count]
                .iter()
                .flatten()
                .find(|export| export.name() == name)
                .map(|export| (export.address, entry.id))
        })
    }

    pub fn resolve_symbol_with_provider(&self, name: &str) -> Option<(u64, Option<u32>)> {
        self.resolve(name)
            .map(|(address, provider)| (address, Some(provider)))
    }

    pub fn unload<P: ModulePlatform>(
        &mut self,
        id: u32,
        platform: &mut P,
    ) -> Result<(), ModuleError> {
        let slot = self
            .entries
            .iter()
            .position(|entry| entry.as_ref().map(|value| value.id) == Some(id))
            .ok_or(ModuleError::UnknownModule)?;
        let entry = self.entries[slot]
            .as_ref()
            .ok_or(ModuleError::UnknownModule)?;
        if entry.dependents != 0 {
            return Err(ModuleError::Busy);
        }
        entry.module.quiesce(platform)?;
        if let Some(exit) = entry.module.exit_address {
            platform.invoke_exit(exit)?;
        }
        let entry = self.entries[slot]
            .take()
            .ok_or(ModuleError::UnknownModule)?;
        for provider in entry.module.dependencies[..entry.module.dependency_count]
            .iter()
            .copied()
        {
            if let Some(provider) = self
                .entries
                .iter_mut()
                .flatten()
                .find(|value| value.id == provider)
            {
                provider.dependents = provider.dependents.saturating_sub(1);
            }
        }
        entry.module.release_pages_quiesced(platform)
    }
}

impl<const MODULES: usize> Default for ModuleRegistry<MODULES> {
    fn default() -> Self {
        Self::new()
    }
}

impl RegistryEntry {
    fn module_name(&self) -> &str {
        core::str::from_utf8(&self.name[..usize::from(self.name_len)]).unwrap_or("")
    }
}

impl LoadedModule {
    pub fn export_count(&self) -> usize {
        self.export_count
    }

    pub fn export(&self, index: usize) -> Option<(&str, u64)> {
        self.exports
            .get(index)?
            .as_ref()
            .map(|entry| (entry.name(), entry.address))
    }

    pub fn add_dependency(&mut self, provider_id: u32) -> Result<(), ModuleError> {
        if provider_id == 0 {
            return Err(ModuleError::Dependency);
        }
        if self.dependencies[..self.dependency_count].contains(&provider_id) {
            return Ok(());
        }
        if self.dependency_count == self.dependencies.len() {
            return Err(ModuleError::Capacity);
        }
        self.dependencies[self.dependency_count] = provider_id;
        self.dependency_count += 1;
        Ok(())
    }

    pub fn unload<P: ModulePlatform>(self, platform: &mut P) -> Result<(), ModuleError> {
        self.quiesce(platform)?;
        self.release_quiesced(platform)
    }

    fn quiesce<P: ModulePlatform>(&self, platform: &mut P) -> Result<(), ModuleError> {
        for allocation in self.allocations[..self.allocation_count].iter().flatten() {
            platform.quiesce(allocation.address, allocation.size)?;
        }
        Ok(())
    }

    fn release_quiesced<P: ModulePlatform>(self, platform: &mut P) -> Result<(), ModuleError> {
        if let Some(exit) = self.exit_address {
            platform.invoke_exit(exit)?;
        }
        self.release_pages_quiesced(platform)
    }

    fn release_pages_quiesced<P: ModulePlatform>(
        self,
        platform: &mut P,
    ) -> Result<(), ModuleError> {
        for allocation in self.allocations[..self.allocation_count]
            .iter()
            .rev()
            .flatten()
        {
            platform.free(allocation.address, allocation.size);
        }
        Ok(())
    }
}

fn load_inner<P: ModulePlatform>(
    platform: &mut P,
    image: &[u8],
) -> Result<LoadedModule, ModuleError> {
    if image.len() < ELF_HEADER_SIZE {
        return Err(ModuleError::Truncated);
    }
    if &image[..4] != b"\x7fELF" || image[4] != 2 || image[5] != 1 || image[6] != 1 {
        return Err(ModuleError::InvalidElf);
    }
    if u16_at(image, 16)? != 1 || u16_at(image, 18)? != 62 || u32_at(image, 20)? != 1 {
        return Err(ModuleError::Unsupported);
    }
    let shoff = usize_at(u64_at(image, 40)?)?;
    let shentsize = u16_at(image, 58)? as usize;
    let shnum = u16_at(image, 60)? as usize;
    let shstrndx = u16_at(image, 62)? as usize;
    if shentsize != SECTION_HEADER_SIZE || shnum == 0 || shnum > 256 || shstrndx >= shnum {
        return Err(ModuleError::InvalidElf);
    }
    checked_range(
        image,
        shoff,
        shentsize.checked_mul(shnum).ok_or(ModuleError::Bounds)?,
    )?;
    let mut sections = [Section {
        kind: 0,
        flags: 0,
        offset: 0,
        size: 0,
        link: 0,
        info: 0,
        align: 1,
        entsize: 0,
    }; 256];
    for (idx, section) in sections.iter_mut().take(shnum).enumerate() {
        let at = shoff
            .checked_add(idx * shentsize)
            .ok_or(ModuleError::Bounds)?;
        *section = Section {
            kind: u32_at(image, at + 4)?,
            flags: u64_at(image, at + 8)?,
            offset: usize_at(u64_at(image, at + 24)?)?,
            size: usize_at(u64_at(image, at + 32)?)?,
            link: u32_at(image, at + 40)? as usize,
            info: u32_at(image, at + 44)? as usize,
            align: usize_at(u64_at(image, at + 48)?)?.max(1),
            entsize: usize_at(u64_at(image, at + 56)?)?,
        };
        if section.kind != 8 {
            checked_range(image, section.offset, section.size)?;
        }
        if !section.align.is_power_of_two() {
            return Err(ModuleError::InvalidElf);
        }
    }
    let names = section_bytes(image, sections[shstrndx])?;
    if sections[shstrndx].kind != SHT_STRTAB {
        return Err(ModuleError::InvalidElf);
    }
    let mut placements = [0u64; 256];
    let mut allocations = [None; 64];
    let mut allocation_count = 0;
    let mut exports: [Option<OwnedModuleExport>; MAX_MODULE_EXPORTS] = [None; MAX_MODULE_EXPORTS];
    let mut export_count = 0;
    let mut dependencies = [0u32; 64];
    let mut dependency_count = 0;
    for idx in 0..shnum {
        let section = sections[idx];
        if section.flags & SHF_ALLOC == 0 || section.size == 0 {
            continue;
        }
        if section.flags & (SHF_WRITE | SHF_EXECINSTR) == SHF_WRITE | SHF_EXECINSTR {
            rollback(platform, &allocations, allocation_count);
            return Err(ModuleError::WritableExecutable);
        }
        if allocation_count == allocations.len() {
            rollback(platform, &allocations, allocation_count);
            return Err(ModuleError::Capacity);
        }
        let address = match platform.allocate_rw_nx(section.size, section.align) {
            Ok(v) => v,
            Err(e) => {
                rollback(platform, &allocations, allocation_count);
                return Err(e);
            }
        };
        placements[idx] = address;
        allocations[allocation_count] = Some(Allocation {
            address,
            size: section.size,
        });
        allocation_count += 1;
        if section.kind != 8 {
            if let Err(e) = platform.write(address, section_bytes(image, section)?) {
                rollback(platform, &allocations, allocation_count);
                return Err(e);
            }
        } else if let Err(e) = platform.zero(address, section.size) {
            rollback(platform, &allocations, allocation_count);
            return Err(e);
        }
    }
    let mut init = None;
    let mut exit = None;
    let apply = (|| {
        for idx in 0..shnum {
            let symtab = sections[idx];
            if symtab.kind != SHT_SYMTAB {
                continue;
            }
            if symtab.entsize != SYMBOL_SIZE
                || symtab.size % SYMBOL_SIZE != 0
                || symtab.link >= shnum
            {
                return Err(ModuleError::InvalidElf);
            }
            if sections[symtab.link].kind != SHT_STRTAB {
                return Err(ModuleError::InvalidElf);
            }
            let strings = section_bytes(image, sections[symtab.link])?;
            let symbols = section_bytes(image, symtab)?;
            for symbol in symbols.chunks(SYMBOL_SIZE) {
                let name_offset = u32_at(symbol, 0)? as usize;
                let info = *symbol.get(4).ok_or(ModuleError::Bounds)?;
                let binding = info >> 4;
                let section_index = u16_at(symbol, 6)?;
                let value = u64_at(symbol, 8)?;
                let name = cstr(strings, name_offset)?;
                if name.is_empty()
                    || (binding != STB_GLOBAL && binding != STB_WEAK)
                    || section_index == SHN_UNDEF
                {
                    continue;
                }
                if name.len() > MODULE_SYMBOL_NAME_MAX {
                    return Err(ModuleError::Capacity);
                }
                let address =
                    resolve_value(section_index, value, &placements, shnum, name, platform)?;
                if exports[..export_count]
                    .iter()
                    .flatten()
                    .any(|entry| entry.name() == name)
                {
                    return Err(ModuleError::InvalidExportTable);
                }
                if export_count == exports.len() {
                    return Err(ModuleError::Capacity);
                }
                let mut owned_name = [0; MODULE_SYMBOL_NAME_MAX];
                owned_name[..name.len()].copy_from_slice(name.as_bytes());
                exports[export_count] = Some(OwnedModuleExport {
                    name: owned_name,
                    name_len: name.len().try_into().map_err(|_| ModuleError::Capacity)?,
                    address,
                });
                export_count += 1;
            }
            for rel_idx in 0..shnum {
                let rela = sections[rel_idx];
                if rela.kind != SHT_RELA || rela.link != idx {
                    continue;
                }
                if rela.entsize != RELA_SIZE || rela.info >= shnum || rela.size % RELA_SIZE != 0 {
                    return Err(ModuleError::InvalidElf);
                }
                let target = sections[rela.info];
                let target_base = placements[rela.info];
                if target_base == 0 {
                    return Err(ModuleError::Relocation);
                }
                for entry in section_bytes(image, rela)?.chunks(RELA_SIZE) {
                    let offset = usize_at(u64_at(entry, 0)?)?;
                    let info = u64_at(entry, 8)?;
                    let sym = usize_at(info >> 32)?;
                    let kind = info as u32;
                    let addend = i64_at(entry, 16)?;
                    let place = target_base
                        .checked_add(offset as u64)
                        .ok_or(ModuleError::Bounds)?;
                    let relocation_width = match kind {
                        1 | 6 | 7 => 8,
                        2 | 4 | 10 | 11 => 4,
                        0 => 0,
                        _ => return Err(ModuleError::Unsupported),
                    };
                    if offset
                        .checked_add(relocation_width)
                        .ok_or(ModuleError::Bounds)?
                        > target.size
                    {
                        return Err(ModuleError::Bounds);
                    }
                    let symbol_at = sym.checked_mul(SYMBOL_SIZE).ok_or(ModuleError::Bounds)?;
                    let symbol = symbols
                        .get(symbol_at..symbol_at + SYMBOL_SIZE)
                        .ok_or(ModuleError::Bounds)?;
                    let name_offset = u32_at(symbol, 0)? as usize;
                    let value = u64_at(symbol, 8)?;
                    let section_index = u16_at(symbol, 6)?;
                    let name = cstr(strings, name_offset)?;
                    if section_index == SHN_UNDEF && !name.is_empty() {
                        if let Some((_, Some(provider))) =
                            platform.resolve_symbol_with_provider(name)
                        {
                            if !dependencies[..dependency_count].contains(&provider) {
                                if dependency_count == dependencies.len() {
                                    return Err(ModuleError::Capacity);
                                }
                                dependencies[dependency_count] = provider;
                                dependency_count += 1;
                            }
                        }
                    }
                    if name == "init_module" || name == "module_init" {
                        init = Some(resolve_value(
                            section_index,
                            value,
                            &placements,
                            shnum,
                            name,
                            platform,
                        )?);
                    } else if name == "cleanup_module" || name == "module_exit" {
                        exit = Some(resolve_value(
                            section_index,
                            value,
                            &placements,
                            shnum,
                            name,
                            platform,
                        )?);
                    }
                    let binding = symbol.get(4).copied().ok_or(ModuleError::Bounds)? >> 4;
                    let symbol_value = resolve_symbol_value(
                        section_index,
                        binding,
                        value,
                        &placements,
                        shnum,
                        name,
                        platform,
                    )?;
                    match kind {
                        1 | 6 | 7 => {
                            let relocated = (symbol_value as i128) + (addend as i128);
                            if relocated < 0 || relocated > u64::MAX as i128 {
                                return Err(ModuleError::Relocation);
                            }
                            platform.write_u64(place, relocated as u64)?;
                        }
                        2 | 4 => {
                            let relocated = symbol_value as i128 + addend as i128 - place as i128;
                            let value: i32 =
                                relocated.try_into().map_err(|_| ModuleError::Relocation)?;
                            platform.write_u32(place, value as u32)?;
                        }
                        10 | 11 => {
                            let relocated = symbol_value as i128 + addend as i128;
                            let value: u32 = if kind == 10 {
                                relocated.try_into().map_err(|_| ModuleError::Relocation)?
                            } else {
                                let v: i32 =
                                    relocated.try_into().map_err(|_| ModuleError::Relocation)?;
                                v as u32
                            };
                            platform.write_u32(place, value)?;
                        }
                        0 => {}
                        _ => return Err(ModuleError::Unsupported),
                    }
                }
            }
        }
        for idx in 0..shnum {
            let section = sections[idx];
            if section.flags & SHF_ALLOC == 0 || section.size == 0 {
                continue;
            }
            platform.protect(
                placements[idx],
                section.size,
                SectionPermissions {
                    writable: section.flags & SHF_WRITE != 0,
                    executable: section.flags & SHF_EXECINSTR != 0,
                },
            )?;
        }
        if let Some(entry) = init {
            if platform.invoke_init(entry)? != 0 {
                return Err(ModuleError::InitFailed);
            }
        }
        Ok(())
    })();
    if let Err(error) = apply {
        rollback(platform, &allocations, allocation_count);
        return Err(error);
    }
    let _ = names;
    Ok(LoadedModule {
        init_address: init,
        exit_address: exit,
        allocations,
        allocation_count,
        exports,
        export_count,
        dependencies,
        dependency_count,
    })
}

fn resolve_value<P: ModulePlatform>(
    index: u16,
    value: u64,
    placements: &[u64; 256],
    count: usize,
    name: &str,
    platform: &P,
) -> Result<u64, ModuleError> {
    match index {
        SHN_UNDEF if name.is_empty() => Ok(0),
        SHN_UNDEF => platform
            .resolve_symbol(name)
            .ok_or(ModuleError::MissingSymbol),
        SHN_ABS => Ok(value),
        section if section as usize >= count => Err(ModuleError::Bounds),
        section => placements[section as usize]
            .checked_add(value)
            .ok_or(ModuleError::Bounds),
    }
}

fn resolve_symbol_value<P: ModulePlatform>(
    index: u16,
    binding: u8,
    value: u64,
    placements: &[u64; 256],
    count: usize,
    name: &str,
    platform: &P,
) -> Result<u64, ModuleError> {
    if index != SHN_UNDEF {
        return resolve_value(index, value, placements, count, name, platform);
    }
    if name.is_empty() {
        return Ok(0);
    }
    if binding == STB_LOCAL {
        return Err(ModuleError::MissingSymbol);
    }
    match platform.resolve_symbol(name) {
        Some(address) => Ok(address),
        None if binding == STB_WEAK => Ok(0),
        None if binding == STB_GLOBAL => Err(ModuleError::MissingSymbol),
        None => Err(ModuleError::Unsupported),
    }
}

fn rollback<P: ModulePlatform>(
    platform: &mut P,
    allocations: &[Option<Allocation>; 64],
    count: usize,
) {
    for allocation in allocations[..count].iter().rev().flatten() {
        platform.free(allocation.address, allocation.size);
    }
}
fn section_bytes(image: &[u8], section: Section) -> Result<&[u8], ModuleError> {
    checked_range(image, section.offset, section.size)
}
fn checked_range(image: &[u8], offset: usize, size: usize) -> Result<&[u8], ModuleError> {
    image
        .get(offset..offset.checked_add(size).ok_or(ModuleError::Bounds)?)
        .ok_or(ModuleError::Truncated)
}
fn cstr(bytes: &[u8], offset: usize) -> Result<&str, ModuleError> {
    let tail = bytes.get(offset..).ok_or(ModuleError::Bounds)?;
    let end = tail
        .iter()
        .position(|b| *b == 0)
        .ok_or(ModuleError::InvalidElf)?;
    core::str::from_utf8(&tail[..end]).map_err(|_| ModuleError::InvalidElf)
}
fn u16_at(b: &[u8], o: usize) -> Result<u16, ModuleError> {
    Ok(u16::from_le_bytes(
        checked_range(b, o, 2)?
            .try_into()
            .map_err(|_| ModuleError::Truncated)?,
    ))
}
fn u32_at(b: &[u8], o: usize) -> Result<u32, ModuleError> {
    Ok(u32::from_le_bytes(
        checked_range(b, o, 4)?
            .try_into()
            .map_err(|_| ModuleError::Truncated)?,
    ))
}
fn u64_at(b: &[u8], o: usize) -> Result<u64, ModuleError> {
    Ok(u64::from_le_bytes(
        checked_range(b, o, 8)?
            .try_into()
            .map_err(|_| ModuleError::Truncated)?,
    ))
}
fn i64_at(b: &[u8], o: usize) -> Result<i64, ModuleError> {
    Ok(i64::from_le_bytes(
        checked_range(b, o, 8)?
            .try_into()
            .map_err(|_| ModuleError::Truncated)?,
    ))
}
fn usize_at(v: u64) -> Result<usize, ModuleError> {
    v.try_into().map_err(|_| ModuleError::Bounds)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Platform {
        next: u64,
        freed: usize,
        last_write: Option<u64>,
        linker: KernelDynamicLinker<'static>,
    }

    static EXPORTS: [KernelExport; 2] = [
        KernelExport {
            name: "kernel_log",
            address: 0xffff_8000_0000_1000,
        },
        KernelExport {
            name: "memory_query",
            address: 0xffff_8000_0000_2000,
        },
    ];
    static MODULE_EXPORTS: [ModuleExport; 2] = [
        ModuleExport {
            module: "net_driver",
            name: "net_send",
            address: 0xffff_a000_0000_1000,
        },
        ModuleExport {
            module: "net_driver",
            name: "kernel_log",
            address: 0xffff_a000_0000_2000,
        },
    ];

    impl Default for Platform {
        fn default() -> Self {
            Self {
                next: 0,
                freed: 0,
                last_write: None,
                linker: KernelDynamicLinker {
                    kernel: KernelSymbolTable::new(&EXPORTS).unwrap(),
                    modules: ModuleSymbolTable::new(&MODULE_EXPORTS).unwrap(),
                },
            }
        }
    }
    impl ModulePlatform for Platform {
        fn allocate_rw_nx(&mut self, _: usize, _: usize) -> Result<u64, ModuleError> {
            self.next += 0x1000;
            Ok(self.next)
        }
        fn write(&mut self, _: u64, _: &[u8]) -> Result<(), ModuleError> {
            Ok(())
        }
        fn zero(&mut self, _: u64, _: usize) -> Result<(), ModuleError> {
            Ok(())
        }
        fn read_u64(&mut self, _: u64) -> Result<u64, ModuleError> {
            Ok(0)
        }
        fn write_u64(&mut self, _: u64, value: u64) -> Result<(), ModuleError> {
            self.last_write = Some(value);
            Ok(())
        }
        fn write_u32(&mut self, _: u64, _: u32) -> Result<(), ModuleError> {
            Ok(())
        }
        fn protect(&mut self, _: u64, _: usize, p: SectionPermissions) -> Result<(), ModuleError> {
            if p.writable && p.executable {
                Err(ModuleError::WritableExecutable)
            } else {
                Ok(())
            }
        }
        fn resolve_symbol(&self, name: &str) -> Option<u64> {
            self.linker.resolve(name)
        }
        fn invoke_init(&mut self, _: u64) -> Result<i32, ModuleError> {
            Ok(0)
        }
        fn invoke_exit(&mut self, _: u64) -> Result<(), ModuleError> {
            Ok(())
        }
        fn quiesce(&mut self, _: u64, _: usize) -> Result<(), ModuleError> {
            Ok(())
        }
        fn free(&mut self, _: u64, _: usize) {
            self.freed += 1;
        }
    }

    fn empty_relocatable() -> [u8; 193] {
        let mut elf = [0u8; 193];
        elf[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        elf[16..18].copy_from_slice(&1u16.to_le_bytes());
        elf[18..20].copy_from_slice(&62u16.to_le_bytes());
        elf[20..24].copy_from_slice(&1u32.to_le_bytes());
        elf[40..48].copy_from_slice(&64u64.to_le_bytes());
        elf[58..60].copy_from_slice(&64u16.to_le_bytes());
        elf[60..62].copy_from_slice(&2u16.to_le_bytes());
        elf[62..64].copy_from_slice(&1u16.to_le_bytes());
        elf[128 + 4..128 + 8].copy_from_slice(&SHT_STRTAB.to_le_bytes());
        elf[128 + 24..128 + 32].copy_from_slice(&192u64.to_le_bytes());
        elf[128 + 32..128 + 40].copy_from_slice(&1u64.to_le_bytes());
        elf[128 + 48..128 + 56].copy_from_slice(&1u64.to_le_bytes());
        elf
    }

    fn relocatable_with_export() -> [u8; 474] {
        let mut elf = [0u8; 474];
        elf[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        elf[16..18].copy_from_slice(&1u16.to_le_bytes());
        elf[18..20].copy_from_slice(&62u16.to_le_bytes());
        elf[20..24].copy_from_slice(&1u32.to_le_bytes());
        elf[40..48].copy_from_slice(&64u64.to_le_bytes());
        elf[58..60].copy_from_slice(&64u16.to_le_bytes());
        elf[60..62].copy_from_slice(&5u16.to_le_bytes());
        elf[62..64].copy_from_slice(&1u16.to_le_bytes());
        let headers = [
            (1usize, SHT_STRTAB, 0u64, 384u64, 33u64, 0u32, 1u64, 0u64),
            (2, 1, SHF_ALLOC | SHF_EXECINSTR, 420, 1, 0, 1, 0),
            (3, SHT_SYMTAB, 0, 421, 48, 4, 8, SYMBOL_SIZE as u64),
            (4, SHT_STRTAB, 0, 469, 5, 0, 1, 0),
        ];
        for (index, kind, flags, offset, size, link, align, entsize) in headers {
            let at = 64 + index * SECTION_HEADER_SIZE;
            elf[at + 4..at + 8].copy_from_slice(&kind.to_le_bytes());
            elf[at + 8..at + 16].copy_from_slice(&flags.to_le_bytes());
            elf[at + 24..at + 32].copy_from_slice(&offset.to_le_bytes());
            elf[at + 32..at + 40].copy_from_slice(&size.to_le_bytes());
            elf[at + 40..at + 44].copy_from_slice(&link.to_le_bytes());
            elf[at + 48..at + 56].copy_from_slice(&align.to_le_bytes());
            elf[at + 56..at + 64].copy_from_slice(&entsize.to_le_bytes());
        }
        elf[384..417].copy_from_slice(b"\0.shstrtab\0.text\0.symtab\0.strtab\0");
        elf[420] = 0xc3;
        elf[421 + SYMBOL_SIZE..421 + SYMBOL_SIZE + 4].copy_from_slice(&1u32.to_le_bytes());
        elf[421 + SYMBOL_SIZE + 4] = 0x12;
        elf[421 + SYMBOL_SIZE + 6..421 + SYMBOL_SIZE + 8].copy_from_slice(&2u16.to_le_bytes());
        elf[421 + SYMBOL_SIZE + 16..421 + SYMBOL_SIZE + 24].copy_from_slice(&1u64.to_le_bytes());
        elf[469..474].copy_from_slice(b"\0foo\0");
        elf
    }

    fn relocatable_with_undefined(binding: u8) -> [u8; 604] {
        let mut elf = [0u8; 604];
        elf[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        elf[16..18].copy_from_slice(&1u16.to_le_bytes());
        elf[18..20].copy_from_slice(&62u16.to_le_bytes());
        elf[20..24].copy_from_slice(&1u32.to_le_bytes());
        elf[40..48].copy_from_slice(&64u64.to_le_bytes());
        elf[58..60].copy_from_slice(&64u16.to_le_bytes());
        elf[60..62].copy_from_slice(&6u16.to_le_bytes());
        elf[62..64].copy_from_slice(&1u16.to_le_bytes());
        let headers = [
            (1usize, SHT_STRTAB, 0u64, 448u64, 64u64, 0u32, 1u64, 0u64),
            (2, 1, SHF_ALLOC | SHF_EXECINSTR, 512, 8, 0, 8, 0),
            (3, SHT_RELA, 0, 520, 24, 4, 8, RELA_SIZE as u64),
            (4, SHT_SYMTAB, 0, 544, 48, 5, 8, SYMBOL_SIZE as u64),
            (5, SHT_STRTAB, 0, 592, 12, 0, 1, 0),
        ];
        for (index, kind, flags, offset, size, link, align, entsize) in headers {
            let at = 64 + index * SECTION_HEADER_SIZE;
            elf[at + 4..at + 8].copy_from_slice(&kind.to_le_bytes());
            elf[at + 8..at + 16].copy_from_slice(&flags.to_le_bytes());
            elf[at + 24..at + 32].copy_from_slice(&offset.to_le_bytes());
            elf[at + 32..at + 40].copy_from_slice(&size.to_le_bytes());
            elf[at + 40..at + 44].copy_from_slice(&link.to_le_bytes());
            elf[at + 44..at + 48]
                .copy_from_slice(&(if index == 3 { 2u32 } else { 0 }).to_le_bytes());
            elf[at + 48..at + 56].copy_from_slice(&align.to_le_bytes());
            elf[at + 56..at + 64].copy_from_slice(&entsize.to_le_bytes());
        }
        elf[448..448 + b"\0.shstrtab\0.text\0.rela.text\0.symtab\0.strtab\0".len()]
            .copy_from_slice(b"\0.shstrtab\0.text\0.rela.text\0.symtab\0.strtab\0");
        elf[520 + 8..520 + 16].copy_from_slice(&((1u64 << 32) | 1).to_le_bytes());
        elf[544 + SYMBOL_SIZE..544 + SYMBOL_SIZE + 4].copy_from_slice(&1u32.to_le_bytes());
        elf[544 + SYMBOL_SIZE + 4] = binding << 4;
        elf[592..604].copy_from_slice(b"\0kernel_log\0");
        elf
    }

    #[test]
    fn accepts_bounded_empty_et_rel_and_unloads() {
        let mut platform = Platform::default();
        let loaded = load(&mut platform, &empty_relocatable()).expect("ELF mínimo válido");
        assert_eq!(loaded.init_address, None);
        assert_eq!(loaded.unload(&mut platform), Ok(()));
        assert_eq!(platform.freed, 0);
    }

    #[test]
    fn rejects_non_elf_and_truncated_section_table() {
        let mut platform = Platform::default();
        assert!(matches!(
            load(&mut platform, b"nope"),
            Err(ModuleError::Truncated)
        ));
        let mut elf = empty_relocatable();
        elf[40..48].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(matches!(
            load(&mut platform, &elf),
            Err(ModuleError::Bounds)
        ));
    }

    #[test]
    fn export_table_resolves_only_validated_sorted_exports() {
        let table = KernelSymbolTable::new(&EXPORTS).unwrap();
        assert_eq!(table.resolve("kernel_log"), Some(0xffff_8000_0000_1000));
        assert_eq!(table.resolve("not_exported"), None);
        assert!(matches!(
            KernelSymbolTable::new(&[
                KernelExport {
                    name: "z",
                    address: 1
                },
                KernelExport {
                    name: "a",
                    address: 2
                },
            ]),
            Err(ModuleError::InvalidExportTable)
        ));
    }

    #[test]
    fn undefined_global_and_weak_symbols_follow_elf_resolution_rules() {
        let platform = Platform::default();
        let placements = [0; 256];
        assert_eq!(
            resolve_symbol_value(
                SHN_UNDEF,
                STB_GLOBAL,
                0,
                &placements,
                1,
                "kernel_log",
                &platform,
            ),
            Ok(0xffff_8000_0000_1000)
        );
        assert_eq!(
            resolve_symbol_value(
                SHN_UNDEF,
                STB_GLOBAL,
                0,
                &placements,
                1,
                "missing_global",
                &platform,
            ),
            Err(ModuleError::MissingSymbol)
        );
        assert_eq!(
            resolve_symbol_value(
                SHN_UNDEF,
                STB_WEAK,
                0,
                &placements,
                1,
                "missing_weak",
                &platform,
            ),
            Ok(0)
        );
        assert_eq!(
            platform.linker.resolve("net_send"),
            Some(0xffff_a000_0000_1000)
        );
        assert_eq!(
            platform.linker.resolve("kernel_log"),
            Some(0xffff_8000_0000_1000)
        );
    }

    #[test]
    fn module_exports_reject_duplicate_symbol_names_per_module() {
        let duplicate = [
            ModuleExport {
                module: "first",
                name: "shared",
                address: 1,
            },
            ModuleExport {
                module: "first",
                name: "shared",
                address: 2,
            },
        ];
        assert_eq!(
            ModuleSymbolTable::new(&duplicate).err(),
            Some(ModuleError::InvalidExportTable)
        );
    }

    #[test]
    fn registry_tracks_dependencies_and_blocks_provider_unload() {
        let mut platform = Platform::default();
        let provider = load(&mut platform, &empty_relocatable()).expect("provider");
        let mut registry = ModuleRegistry::<4>::new();
        let provider_id = registry
            .publish("provider", provider)
            .expect("publish provider");
        let mut dependent = load(&mut platform, &empty_relocatable()).expect("dependent");
        dependent
            .add_dependency(provider_id)
            .expect("record provider");
        dependent
            .add_dependency(provider_id)
            .expect("deduplicate provider");
        let dependent_id = registry
            .publish("dependent", dependent)
            .expect("publish dependent");
        assert_eq!(
            registry.unload(provider_id, &mut platform),
            Err(ModuleError::Busy)
        );
        assert_eq!(registry.unload(dependent_id, &mut platform), Ok(()));
        assert_eq!(registry.unload(provider_id, &mut platform), Ok(()));
        assert_eq!(
            registry.unload(provider_id, &mut platform),
            Err(ModuleError::UnknownModule)
        );
    }

    #[test]
    fn extracts_defined_global_exports_from_real_et_rel_symtab() {
        let mut platform = Platform::default();
        let loaded = load(&mut platform, &relocatable_with_export()).expect("ET_REL export");
        assert_eq!(loaded.export_count(), 1);
        assert_eq!(loaded.export(0), Some(("foo", 0x1000)));
        let mut registry = ModuleRegistry::<2>::new();
        let id = registry.publish("fixture", loaded).expect("publish");
        assert_eq!(registry.resolve("foo"), Some((0x1000, id)));
        assert_eq!(registry.unload(id, &mut platform), Ok(()));
    }

    #[test]
    fn loader_relocates_present_global_and_handles_missing_global_and_weak() {
        let mut platform = Platform::default();
        let image = relocatable_with_undefined(STB_GLOBAL);
        let loaded = load(&mut platform, &image).expect("strong symbol resolution");
        assert_eq!(platform.last_write, Some(0xffff_8000_0000_1000));
        loaded.unload(&mut platform).expect("unload");

        let mut missing = Platform::default();
        let mut absent = relocatable_with_undefined(STB_GLOBAL);
        absent[592..600].copy_from_slice(b"\0absent\0");
        assert_eq!(
            load(&mut missing, &absent).err(),
            Some(ModuleError::MissingSymbol)
        );
        assert_eq!(missing.freed, 1);

        let mut weak = Platform::default();
        let mut weak_image = relocatable_with_undefined(STB_WEAK);
        weak_image[592..600].copy_from_slice(b"\0absent\0");
        let loaded = load(&mut weak, &weak_image).expect("missing weak is zero");
        assert_eq!(weak.last_write, Some(0));
        loaded.unload(&mut weak).expect("unload");

        let mut failed = Platform::default();
        let mut unsupported = relocatable_with_undefined(STB_GLOBAL);
        unsupported[520 + 8..520 + 16].copy_from_slice(&((1u64 << 32) | 0xffff).to_le_bytes());
        assert_eq!(
            load(&mut failed, &unsupported).err(),
            Some(ModuleError::Unsupported)
        );
        assert_eq!(failed.freed, 1);
    }
}

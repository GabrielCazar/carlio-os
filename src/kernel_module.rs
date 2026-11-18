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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleError {
    Truncated,
    InvalidElf,
    Unsupported,
    Bounds,
    WritableExecutable,
    Allocation,
    MissingSymbol,
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
    fn invoke_init(&mut self, address: u64) -> Result<i32, ModuleError>;
    fn invoke_exit(&mut self, address: u64) -> Result<(), ModuleError>;
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
    pub dependency_count: usize,
}

impl LoadedModule {
    pub fn unload<P: ModulePlatform>(self, platform: &mut P) -> Result<(), ModuleError> {
        if self.dependency_count != 0 {
            return Err(ModuleError::Busy);
        }
        if let Some(exit) = self.exit_address {
            platform.invoke_exit(exit)?;
        }
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
                    let binding = symbol.get(4).copied().ok_or(ModuleError::Bounds)? & 0x0f;
                    let symbol_value = if section_index == SHN_UNDEF && binding == 2 {
                        platform.resolve_symbol(name).unwrap_or_default()
                    } else {
                        resolve_value(section_index, value, &placements, shnum, name, platform)?
                    };
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
        dependency_count: 0,
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

    #[derive(Default)]
    struct Platform {
        next: u64,
        freed: usize,
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
        fn write_u64(&mut self, _: u64, _: u64) -> Result<(), ModuleError> {
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
        fn resolve_symbol(&self, _: &str) -> Option<u64> {
            None
        }
        fn invoke_init(&mut self, _: u64) -> Result<i32, ModuleError> {
            Ok(0)
        }
        fn invoke_exit(&mut self, _: u64) -> Result<(), ModuleError> {
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
}

//! Núcleo de relocations ELF64 x86_64 para o linker dinâmico.
//!
//! O resolvedor de símbolos e o carregamento de objetos compartilhados são
//! fornecidos pelo chamador; esta camada valida destinos e aplica as formas
//! básicas usadas por executáveis PIE e PLT/GOT.

use crate::elf::{ElfLoadTarget, LoadSegment, PF_W, USER_TOP};

pub const R_X86_64_64: u32 = 1;
pub const R_X86_64_RELATIVE: u32 = 8;
pub const R_X86_64_GLOB_DAT: u32 = 6;
pub const R_X86_64_JUMP_SLOT: u32 = 7;

/// GNU symbol version identity. The hash is the ELF SysV hash used by
/// `DT_VERNEED`/`DT_VERDEF`; `hidden` is the high bit from `VERSYM`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SymbolVersion<'a> {
    pub name: &'a str,
    pub hash: u32,
    pub hidden: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SymbolDefinition<'a> {
    pub name: &'a str,
    pub address: u64,
    pub version: Option<SymbolVersion<'a>>,
    pub weak: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SymbolRequest<'a> {
    pub name: &'a str,
    pub version: Option<SymbolVersion<'a>>,
}

/// Busca símbolo respeitando o nome e, quando pedido, o nome/hash da versão.
/// Símbolos fortes têm precedência sobre fracos; versões ocultas não podem
/// satisfazer uma busca sem versão (semântica `dlvsym`/GNU versym).
pub fn resolve_symbol<'a>(
    request: SymbolRequest<'_>,
    definitions: &'a [SymbolDefinition<'a>],
) -> Option<u64> {
    let mut weak = None;
    for definition in definitions {
        if definition.name != request.name {
            continue;
        }
        let matches = match (request.version, definition.version) {
            (Some(wanted), Some(found)) => wanted.name == found.name && wanted.hash == found.hash,
            (Some(_), None) | (None, Some(SymbolVersion { hidden: true, .. })) => false,
            (None, _) => true,
        };
        if !matches {
            continue;
        }
        if !definition.weak {
            return Some(definition.address);
        }
        weak.get_or_insert(definition.address);
    }
    weak
}

/// Adiciona dependências em uma fila bounded, sem duplicatas. O chamador
/// percorre a fila em breadth-first para obter a ordem global de lookup ELF.
pub fn enqueue_dependency<'a, const N: usize>(
    queue: &mut [Option<&'a str>; N],
    length: &mut usize,
    name: &'a str,
) -> Result<(), LinkError> {
    if name.is_empty() || name.contains('/') || name.as_bytes().contains(&0) {
        return Err(LinkError::InvalidAddress);
    }
    if queue[..*length].contains(&Some(name)) {
        return Ok(());
    }
    if *length == N {
        return Err(LinkError::ArithmeticOverflow);
    }
    queue[*length] = Some(name);
    *length += 1;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Relocation {
    /// Endereço virtual antes do bias de carga.
    pub offset: u64,
    pub kind: u32,
    pub symbol_index: u32,
    pub addend: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkError {
    UnsupportedRelocation,
    UndefinedSymbol,
    InvalidAddress,
    ArithmeticOverflow,
    Memory,
}

/// Resolve e aplica relocations RELA de um objeto já mapeado. Para símbolos
/// indefinidos, `resolve` recebe o índice da tabela dinâmica e retorna o valor
/// absoluto do símbolo escolhido pelo linker.
pub fn apply_rela<T, F>(
    target: &mut T,
    segments: &[LoadSegment],
    load_bias: u64,
    relocations: &[Relocation],
    mut resolve: F,
) -> Result<(), LinkError>
where
    T: ElfLoadTarget,
    F: FnMut(u32) -> Option<u64>,
{
    for relocation in relocations {
        let address = relocation
            .offset
            .checked_add(load_bias)
            .ok_or(LinkError::ArithmeticOverflow)?;
        let end = address.checked_add(8).ok_or(LinkError::InvalidAddress)?;
        if end > USER_TOP
            || !segments.iter().any(|segment| {
                segment.flags & PF_W != 0
                    && address >= segment.virtual_address
                    && end <= segment.virtual_address.saturating_add(segment.memory_size)
            })
        {
            return Err(LinkError::InvalidAddress);
        }
        let value = match relocation.kind {
            R_X86_64_RELATIVE if relocation.symbol_index == 0 => i128::from(load_bias)
                .checked_add(i128::from(relocation.addend))
                .and_then(|value| u64::try_from(value).ok())
                .ok_or(LinkError::ArithmeticOverflow)?,
            R_X86_64_64 | R_X86_64_GLOB_DAT | R_X86_64_JUMP_SLOT => {
                let symbol = resolve(relocation.symbol_index).ok_or(LinkError::UndefinedSymbol)?;
                i128::from(symbol)
                    .checked_add(i128::from(relocation.addend))
                    .and_then(|value| u64::try_from(value).ok())
                    .ok_or(LinkError::ArithmeticOverflow)?
            }
            _ => return Err(LinkError::UnsupportedRelocation),
        };
        target
            .write(address, &value.to_le_bytes())
            .map_err(|_| LinkError::Memory)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elf::{ElfLoadTarget, LoadSegment, PF_R, PF_W};

    #[test]
    fn symbol_lookup_honors_versions_strength_and_hidden_bit() {
        let v1 = SymbolVersion {
            name: "GLIBC_2.2.5",
            hash: 0x09691a75,
            hidden: false,
        };
        let v2 = SymbolVersion {
            name: "GLIBC_2.34",
            hash: 0x069691b4,
            hidden: false,
        };
        let defs = [
            SymbolDefinition {
                name: "open",
                address: 0x10,
                version: Some(v1),
                weak: false,
            },
            SymbolDefinition {
                name: "open",
                address: 0x20,
                version: Some(v2),
                weak: true,
            },
            SymbolDefinition {
                name: "open",
                address: 0x30,
                version: Some(SymbolVersion { hidden: true, ..v1 }),
                weak: false,
            },
        ];
        assert_eq!(
            resolve_symbol(
                SymbolRequest {
                    name: "open",
                    version: Some(v1)
                },
                &defs
            ),
            Some(0x10)
        );
        assert_eq!(
            resolve_symbol(
                SymbolRequest {
                    name: "open",
                    version: Some(v2)
                },
                &defs
            ),
            Some(0x20)
        );
        assert_eq!(
            resolve_symbol(
                SymbolRequest {
                    name: "open",
                    version: None
                },
                &defs
            ),
            Some(0x10)
        );
        assert_eq!(
            resolve_symbol(
                SymbolRequest {
                    name: "missing",
                    version: None
                },
                &defs
            ),
            None
        );
    }

    #[test]
    fn dependency_queue_is_bounded_and_deduplicated() {
        let mut queue = [None; 2];
        let mut length = 0;
        enqueue_dependency(&mut queue, &mut length, "libc.so.6").unwrap();
        enqueue_dependency(&mut queue, &mut length, "libc.so.6").unwrap();
        enqueue_dependency(&mut queue, &mut length, "libm.so.6").unwrap();
        assert_eq!(length, 2);
        assert_eq!(
            enqueue_dependency(&mut queue, &mut length, "libdl.so.2"),
            Err(LinkError::ArithmeticOverflow)
        );
        assert_eq!(
            enqueue_dependency(&mut queue, &mut length, "../lib.so"),
            Err(LinkError::InvalidAddress)
        );
    }

    struct Memory([u8; 4096]);
    impl ElfLoadTarget for Memory {
        type Error = ();
        fn map_zeroed_page(&mut self, _: u64) -> Result<(), ()> {
            Ok(())
        }
        fn write(&mut self, address: u64, bytes: &[u8]) -> Result<(), ()> {
            let start = usize::try_from(address - 0x20_1000).map_err(|_| ())?;
            self.0
                .get_mut(start..start + bytes.len())
                .ok_or(())?
                .copy_from_slice(bytes);
            Ok(())
        }
        fn protect_page(&mut self, _: u64, _: u32) -> Result<(), ()> {
            Ok(())
        }
        fn unmap_page(&mut self, _: u64) -> Result<(), ()> {
            Ok(())
        }
    }

    #[test]
    fn applies_relative_and_symbol_plt_relocations() {
        let mut memory = Memory([0; 4096]);
        let segments = [LoadSegment {
            file_offset: 0,
            virtual_address: 0x20_1000,
            file_size: 4096,
            memory_size: 4096,
            flags: PF_R | PF_W,
            alignment: 4096,
        }];
        let relocs = [
            Relocation {
                offset: 0x1100,
                kind: R_X86_64_RELATIVE,
                symbol_index: 0,
                addend: 0x80,
            },
            Relocation {
                offset: 0x1108,
                kind: R_X86_64_JUMP_SLOT,
                symbol_index: 3,
                addend: 4,
            },
        ];
        apply_rela(&mut memory, &segments, 0x20_0000, &relocs, |index| {
            (index == 3).then_some(0x4000)
        })
        .unwrap();
        assert_eq!(
            u64::from_le_bytes(memory.0[0x100..0x108].try_into().unwrap()),
            0x20_0080
        );
        assert_eq!(
            u64::from_le_bytes(memory.0[0x108..0x110].try_into().unwrap()),
            0x4004
        );
    }
}

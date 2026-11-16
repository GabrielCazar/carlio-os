//! Validação de intervalos de usuário antes de copiar dados em syscalls.
use crate::elf::USER_TOP;

pub const PAGE_SIZE: u64 = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserPage {
    pub present: bool,
    pub user: bool,
    pub writable: bool,
}

pub trait UserPageLookup {
    fn page(&self, virtual_page: u64) -> Option<UserPage>;
}

impl<A: crate::memory::PageTableAccess, const N: usize> UserPageLookup
    for crate::memory::AddressSpace<A, N>
{
    fn page(&self, virtual_page: u64) -> Option<UserPage> {
        let mapping = self.mapping(virtual_page).ok()?;
        Some(UserPage {
            present: true,
            user: mapping.flags & crate::memory::PAGE_FLAG_USER != 0,
            writable: mapping.flags & crate::memory::PAGE_FLAG_WRITABLE != 0,
        })
    }
}

impl<A: crate::memory::PageTableAccess> UserPageLookup for crate::memory::PageTableManager<A> {
    fn page(&self, virtual_page: u64) -> Option<UserPage> {
        let mapping = self.mapping(virtual_page).ok()?;
        Some(UserPage {
            present: true,
            user: mapping.flags & crate::memory::PAGE_FLAG_USER != 0,
            writable: mapping.flags & crate::memory::PAGE_FLAG_WRITABLE != 0,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserBufferError {
    AddressOverflow,
    OutsideUserRange,
    Unmapped,
    NotUser,
    ReadOnly,
}

/// Confere todas as páginas tocadas pelo intervalo. Zero bytes aceita qualquer
/// ponteiro canonical user (inclusive NULL), sem dereferenciá-lo.
pub fn validate<L: UserPageLookup>(
    lookup: &L,
    address: u64,
    length: usize,
    write: bool,
) -> Result<(), UserBufferError> {
    if length == 0 {
        return Ok(());
    }
    let end = address
        .checked_add(length as u64)
        .ok_or(UserBufferError::AddressOverflow)?;
    if address < PAGE_SIZE || end > USER_TOP {
        return Err(UserBufferError::OutsideUserRange);
    }
    let mut page = address & !(PAGE_SIZE - 1);
    let last = (end - 1) & !(PAGE_SIZE - 1);
    loop {
        let mapping = lookup.page(page).ok_or(UserBufferError::Unmapped)?;
        if !mapping.present {
            return Err(UserBufferError::Unmapped);
        }
        if !mapping.user {
            return Err(UserBufferError::NotUser);
        }
        if write && !mapping.writable {
            return Err(UserBufferError::ReadOnly);
        }
        if page == last {
            break;
        }
        page = page
            .checked_add(PAGE_SIZE)
            .ok_or(UserBufferError::AddressOverflow)?;
    }
    Ok(())
}

/// Copia bytes do espaço virtual atual para um buffer do kernel após validar
/// todas as páginas. `lookup` deve descrever exatamente o CR3 ativo, a VA deve
/// permanecer mapeada durante a cópia e o chamador deve estar em CPL0.
///
/// # Safety
/// O chamador deve garantir que o CR3 e os mapeamentos descritos por `lookup`
/// permaneçam estáveis durante a cópia e que `destination` seja válido.
pub unsafe fn copy_from_user<L: UserPageLookup>(
    lookup: &L,
    address: u64,
    destination: &mut [u8],
) -> Result<(), UserBufferError> {
    if destination.is_empty() {
        return Ok(());
    }
    validate(lookup, address, destination.len(), false)?;
    unsafe {
        core::ptr::copy_nonoverlapping(
            address as *const u8,
            destination.as_mut_ptr(),
            destination.len(),
        );
    }
    Ok(())
}

/// Copia bytes do kernel para o espaço virtual atual após validar todas as
/// páginas como graváveis. Requer o mesmo contrato de CR3/estabilidade da
/// função `copy_from_user`.
///
/// # Safety
/// O chamador deve garantir que o CR3 e os mapeamentos descritos por `lookup`
/// permaneçam estáveis durante a cópia e que `source` seja válido.
pub unsafe fn copy_to_user<L: UserPageLookup>(
    lookup: &L,
    address: u64,
    source: &[u8],
) -> Result<(), UserBufferError> {
    if source.is_empty() {
        return Ok(());
    }
    validate(lookup, address, source.len(), true)?;
    unsafe {
        core::ptr::copy_nonoverlapping(source.as_ptr(), address as *mut u8, source.len());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Pages;
    impl UserPageLookup for Pages {
        fn page(&self, page: u64) -> Option<UserPage> {
            if (0x400000..0x403000).contains(&page) {
                Some(UserPage {
                    present: true,
                    user: true,
                    writable: page != 0x402000,
                })
            } else {
                None
            }
        }
    }
    #[test]
    fn accepts_valid_read_and_write_ranges() {
        assert_eq!(validate(&Pages, 0x400fff, 2, false), Ok(()));
        assert_eq!(validate(&Pages, 0x400000, 16, true), Ok(()));
    }
    #[test]
    fn checks_every_page_and_permissions() {
        assert_eq!(
            validate(&Pages, 0x401fff, 2, true),
            Err(UserBufferError::ReadOnly)
        );
        assert_eq!(
            validate(&Pages, 0x402fff, 2, false),
            Err(UserBufferError::Unmapped)
        );
    }
    #[test]
    fn rejects_overflow_and_kernel_addresses() {
        assert_eq!(
            validate(&Pages, u64::MAX - 2, 8, false),
            Err(UserBufferError::AddressOverflow)
        );
        assert_eq!(
            validate(&Pages, USER_TOP, 1, false),
            Err(UserBufferError::OutsideUserRange)
        );
    }
}

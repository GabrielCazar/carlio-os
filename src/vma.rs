//! Mapa fixo de regiões virtuais de usuário.
//!
//! Este módulo reserva intervalos e verifica conflitos; a instalação das
//! páginas físicas continua sendo responsabilidade do backend do address
//! space. Separar as duas operações permite que falhas de alocação de frames
//! desfaçam a reserva sem deixar regiões parcialmente publicadas.

use crate::elf::USER_TOP;

pub const PAGE_SIZE: u64 = 4096;
pub const MMAP_BASE: u64 = 0x0000_4000_0000_0000;
pub const USER_HEAP_BASE: u64 = 0x0000_3000_0000_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionKind {
    Mmap,
    Heap,
    Image,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub start: u64,
    /// Endereço exclusivo, sempre alinhado a página.
    pub end: u64,
    pub kind: RegionKind,
    pub protection: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmaError {
    InvalidRange,
    Capacity,
    Overlap,
    NotMapped,
    HeapCollision,
}

/// Tabela ordenada de VMA sem alocação dinâmica, adequada ao caminho syscall.
#[derive(Clone, Copy)]
pub struct VmaMap<const N: usize> {
    regions: [Option<Region>; N],
    count: usize,
    mmap_cursor: u64,
    heap_start: u64,
    heap_end: u64,
    heap_limit: u64,
}

/// Espaço de usuário completo para um processo: metadados VMA e ownership das
/// folhas no seu address space privado. O caller mantém uma instância distinta
/// por processo e não deve compartilhar uma instância durante syscalls.
pub struct ProcessVm<A, const PAGES: usize, const REGIONS: usize> {
    address_space: crate::memory::OwnedAddressSpace<A, PAGES>,
    vmas: VmaMap<REGIONS>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProcessMemoryUsage {
    pub owned_leaf_pages: usize,
    pub private_page_table_pages: usize,
    pub total_owned_pages: usize,
    pub leaf_page_limit: usize,
    /// Cota superior conservadora: folha + três níveis de tabela por folha,
    /// mais o PML4 privado do processo.
    pub maximum_owned_pages: usize,
}

impl<A: crate::memory::PageTableAccess, const PAGES: usize, const REGIONS: usize>
    ProcessVm<A, PAGES, REGIONS>
{
    pub fn new(
        address_space: crate::memory::OwnedAddressSpace<A, PAGES>,
        heap_start: u64,
        heap_limit: u64,
    ) -> Self {
        Self {
            address_space,
            vmas: VmaMap::new(heap_start, heap_limit),
        }
    }

    pub const fn root(&self) -> u64 {
        self.address_space.root()
    }

    /// Folhas de usuário atualmente pertencentes a este processo, incluindo
    /// imagem, stack, heap e mappings anônimos.
    pub const fn owned_user_pages(&self) -> usize {
        self.address_space.owned_user_pages()
    }

    /// Tabelas de página privadas do processo, incluindo o PML4 próprio.
    pub fn owned_page_table_pages(&self) -> Result<usize, crate::memory::FrameError> {
        self.address_space.owned_user_page_table_pages()
    }

    pub fn memory_usage(&self) -> Result<ProcessMemoryUsage, crate::memory::FrameError> {
        let owned_leaf_pages = self.owned_user_pages();
        let private_page_table_pages = self.owned_page_table_pages()?;
        let total_owned_pages = owned_leaf_pages
            .checked_add(private_page_table_pages)
            .ok_or(crate::memory::FrameError::CapacityExceeded)?;
        let maximum_owned_pages = PAGES
            .checked_mul(4)
            .and_then(|pages| pages.checked_add(1))
            .ok_or(crate::memory::FrameError::CapacityExceeded)?;
        Ok(ProcessMemoryUsage {
            owned_leaf_pages,
            private_page_table_pages,
            total_owned_pages,
            leaf_page_limit: self.user_page_capacity(),
            maximum_owned_pages,
        })
    }

    /// Limite estático de folhas do address space deste processo.
    pub const fn user_page_capacity(&self) -> usize {
        PAGES
    }

    pub const fn program_break(&self) -> u64 {
        self.vmas.program_break()
    }

    pub fn mapping(
        &self,
        address: u64,
    ) -> Result<crate::memory::PageMapping, crate::memory::FrameError> {
        self.address_space.mapping(address)
    }

    /// Registra páginas já criadas pelo ELF loader (imagem, TLS ou stack) para
    /// que `mmap` e `brk` não possam sobrepor mapeamentos anteriores.
    pub fn reserve_existing(&mut self, start: u64, length: u64) -> Result<(), VmaError> {
        let start = page_floor(start);
        let length = page_ceil(length).ok_or(VmaError::InvalidRange)?;
        let end = start.checked_add(length).ok_or(VmaError::InvalidRange)?;
        self.vmas.insert(Region {
            start,
            end,
            kind: RegionKind::Image,
            protection: 0,
        })
    }

    /// `MAP_PRIVATE | MAP_ANONYMOUS` sem arquivo, ainda sem page fault
    /// lazy: todas as folhas são alocadas e zeradas antes de publicar a VMA.
    pub fn mmap_anonymous(
        &mut self,
        hint: u64,
        length: u64,
        protection: u8,
    ) -> Result<u64, ProcessVmError> {
        if protection == 0 {
            // x86 page-table support here cannot represent PROT_NONE while
            // retaining a present, user-owned leaf safely.
            return Err(ProcessVmError::UnsupportedMapping);
        }
        let mut staged = self.vmas;
        let address = staged
            .mmap(hint, length, protection)
            .map_err(ProcessVmError::Vma)?;
        let end = staged
            .regions()
            .find(|region| region.start == address && region.kind == RegionKind::Mmap)
            .ok_or(ProcessVmError::Vma(VmaError::NotMapped))?
            .end;
        let flags = page_flags(protection);
        let mut page = address;
        while page < end {
            if let Err(error) = self.address_space.map_zeroed(page, flags) {
                let mut rollback = address;
                while rollback < page {
                    if self.address_space.unmap(rollback).is_err() {
                        self.vmas = staged;
                        return Err(ProcessVmError::RollbackFailed);
                    }
                    rollback += PAGE_SIZE;
                }
                return Err(ProcessVmError::Memory(error));
            }
            page += PAGE_SIZE;
        }
        self.vmas = staged;
        Ok(address)
    }

    /// Atende apenas `MAP_PRIVATE | MAP_ANONYMOUS`; arquivo, compartilhamento
    /// e endereço fixo dependem de backends que ainda não existem.
    pub fn mmap_linux(
        &mut self,
        hint: u64,
        length: u64,
        protection: u64,
        flags: u64,
        descriptor: u64,
        offset: u64,
    ) -> Result<u64, ProcessVmError> {
        const MAP_PRIVATE: u64 = 0x02;
        const MAP_FIXED: u64 = 0x10;
        const MAP_ANONYMOUS: u64 = 0x20;
        if protection & !0x7 != 0 || flags & MAP_FIXED != 0 {
            return Err(ProcessVmError::Vma(VmaError::InvalidRange));
        }
        if flags != MAP_PRIVATE | MAP_ANONYMOUS || descriptor != u64::MAX || offset != 0 {
            return Err(ProcessVmError::UnsupportedMapping);
        }
        let protection =
            u8::try_from(protection).map_err(|_| ProcessVmError::Vma(VmaError::InvalidRange))?;
        self.mmap_anonymous(hint, length, protection)
    }

    pub fn munmap(&mut self, address: u64, length: u64) -> Result<(), ProcessVmError> {
        if !address.is_multiple_of(PAGE_SIZE) {
            return Err(ProcessVmError::Vma(VmaError::InvalidRange));
        }
        let mut staged = self.vmas;
        staged
            .munmap(address, length)
            .map_err(ProcessVmError::Vma)?;
        let start = page_floor(address);
        let aligned_length =
            page_ceil(length).ok_or(ProcessVmError::Vma(VmaError::InvalidRange))?;
        let end = start
            .checked_add(aligned_length)
            .ok_or(ProcessVmError::Vma(VmaError::InvalidRange))?;
        let mut page = start;
        while page < end {
            self.address_space
                .unmap(page)
                .map_err(ProcessVmError::Memory)?;
            page += PAGE_SIZE;
        }
        self.vmas = staged;
        Ok(())
    }

    /// Altera permissões de um intervalo contido em uma única VMA anônima.
    /// W+X e PROT_NONE ficam indisponíveis até que a política e o rollback de
    /// páginas não presentes sejam implementados.
    pub fn mprotect(
        &mut self,
        address: u64,
        length: u64,
        protection: u64,
    ) -> Result<(), ProcessVmError> {
        if !address.is_multiple_of(PAGE_SIZE) || length == 0 || protection & !0x7 != 0 {
            return Err(ProcessVmError::Vma(VmaError::InvalidRange));
        }
        if protection & 0x2 != 0 && protection & 0x4 != 0 {
            return Err(ProcessVmError::PermissionDenied);
        }
        if protection & 0x7 == 0 {
            return Err(ProcessVmError::UnsupportedMapping);
        }
        let protection =
            u8::try_from(protection).map_err(|_| ProcessVmError::Vma(VmaError::InvalidRange))?;
        let aligned_length =
            page_ceil(length).ok_or(ProcessVmError::Vma(VmaError::InvalidRange))?;
        let end = address
            .checked_add(aligned_length)
            .filter(|end| *end <= USER_TOP)
            .ok_or(ProcessVmError::Vma(VmaError::InvalidRange))?;
        let mut staged = self.vmas;
        let index = staged
            .regions()
            .position(|region| {
                region.kind == RegionKind::Mmap && address >= region.start && end <= region.end
            })
            .ok_or(ProcessVmError::Vma(VmaError::NotMapped))?;
        let original = staged
            .regions()
            .nth(index)
            .ok_or(ProcessVmError::Vma(VmaError::NotMapped))?;
        let pieces = 1 + usize::from(address > original.start) + usize::from(end < original.end);
        if staged.count - 1 + pieces > REGIONS {
            return Err(ProcessVmError::Vma(VmaError::Capacity));
        }
        staged.remove(index);
        if original.start < address {
            staged
                .insert(Region {
                    end: address,
                    ..original
                })
                .map_err(ProcessVmError::Vma)?;
        }
        staged
            .insert(Region {
                start: address,
                end,
                protection,
                ..original
            })
            .map_err(ProcessVmError::Vma)?;
        if end < original.end {
            staged
                .insert(Region {
                    start: end,
                    ..original
                })
                .map_err(ProcessVmError::Vma)?;
        }

        let old_flags = page_flags(original.protection);
        let new_flags = page_flags(protection);
        let mut changed = 0usize;
        let mut page = address;
        while page < end {
            if let Err(error) = self.address_space.protect_user(page, new_flags) {
                let mut rollback = address;
                for _ in 0..changed {
                    if self
                        .address_space
                        .protect_user(rollback, old_flags)
                        .is_err()
                    {
                        return Err(ProcessVmError::RollbackFailed);
                    }
                    rollback += PAGE_SIZE;
                }
                return Err(ProcessVmError::Memory(error));
            }
            changed += 1;
            page += PAGE_SIZE;
        }
        self.vmas = staged;
        Ok(())
    }

    pub fn brk(&mut self, requested: u64) -> Result<u64, ProcessVmError> {
        if requested == 0 {
            return Ok(self.vmas.program_break());
        }
        let current = self.vmas.program_break();
        let mut staged = self.vmas;
        let result = staged.brk(requested);
        if result != requested {
            return Ok(current);
        }
        let old_end = page_ceil(current).ok_or(ProcessVmError::Vma(VmaError::InvalidRange))?;
        let new_end = page_ceil(requested).ok_or(ProcessVmError::Vma(VmaError::InvalidRange))?;
        if requested > current {
            let mut page = old_end;
            while page < new_end {
                if let Err(error) = self.address_space.map_zeroed(page, page_flags(0b011)) {
                    while page > old_end {
                        page -= PAGE_SIZE;
                        if self.address_space.unmap(page).is_err() {
                            self.vmas = staged;
                            return Err(ProcessVmError::RollbackFailed);
                        }
                    }
                    let _ = error;
                    return Ok(current);
                }
                page += PAGE_SIZE;
            }
        } else {
            let mut page = new_end;
            while page < old_end {
                self.address_space
                    .unmap(page)
                    .map_err(ProcessVmError::Memory)?;
                page += PAGE_SIZE;
            }
        }
        self.vmas = staged;
        Ok(requested)
    }

    pub fn into_address_space(self) -> crate::memory::OwnedAddressSpace<A, PAGES> {
        self.address_space
    }

    pub fn try_destroy(self) -> Result<A, (Self, crate::memory::FrameError)> {
        let Self {
            address_space,
            vmas,
        } = self;
        match address_space.try_destroy() {
            Ok(access) => Ok(access),
            Err((address_space, error)) => Err((
                Self {
                    address_space,
                    vmas,
                },
                error,
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessVmError {
    Vma(VmaError),
    Memory(crate::memory::FrameError),
    RollbackFailed,
    UnsupportedMapping,
    PermissionDenied,
}

fn page_flags(protection: u8) -> u64 {
    1 | crate::memory::PAGE_FLAG_USER
        | if protection & 0b010 != 0 {
            crate::memory::PAGE_FLAG_WRITABLE
        } else {
            0
        }
        | if protection & 0b100 == 0 {
            crate::memory::PAGE_FLAG_NO_EXECUTE
        } else {
            0
        }
}

impl<const N: usize> VmaMap<N> {
    pub const fn new(heap_start: u64, heap_limit: u64) -> Self {
        Self {
            regions: [None; N],
            count: 0,
            mmap_cursor: MMAP_BASE,
            heap_start,
            heap_end: heap_start,
            heap_limit,
        }
    }

    pub fn regions(&self) -> impl Iterator<Item = Region> + '_ {
        self.regions[..self.count]
            .iter()
            .filter_map(|region| *region)
    }

    pub const fn program_break(&self) -> u64 {
        self.heap_end
    }

    /// Reserva um intervalo alinhado em first-fit circular a partir do cursor.
    pub fn mmap(&mut self, hint: u64, length: u64, protection: u8) -> Result<u64, VmaError> {
        let length = page_ceil(length).ok_or(VmaError::InvalidRange)?;
        if length == 0 || protection & !0b111 != 0 || protection & 0b110 == 0b110 {
            return Err(VmaError::InvalidRange);
        }
        if hint != 0 {
            let start = page_floor(hint);
            if self.range_free(start, length) {
                self.insert(Region {
                    start,
                    end: start.checked_add(length).ok_or(VmaError::InvalidRange)?,
                    kind: RegionKind::Mmap,
                    protection,
                })?;
                self.mmap_cursor = start.checked_add(length).unwrap_or(MMAP_BASE);
                return Ok(start);
            }
        }
        let span = USER_TOP
            .checked_sub(MMAP_BASE)
            .ok_or(VmaError::InvalidRange)?;
        let attempts = self.count.saturating_add(2);
        let mut start = self.mmap_cursor.max(MMAP_BASE);
        for _ in 0..attempts {
            if start.checked_add(length).is_some_and(|end| end <= USER_TOP)
                && self.range_free(start, length)
            {
                self.insert(Region {
                    start,
                    end: start + length,
                    kind: RegionKind::Mmap,
                    protection,
                })?;
                self.mmap_cursor = start.checked_add(length).unwrap_or(MMAP_BASE);
                return Ok(start);
            }
            let next = self
                .regions()
                .filter(|region| region.end > start)
                .map(|region| region.end)
                .min();
            start = match next {
                Some(end) => end,
                None => MMAP_BASE,
            };
            if start == MMAP_BASE || start >= MMAP_BASE + span {
                break;
            }
        }
        Err(VmaError::Capacity)
    }

    /// Libera intervalo completo; divisões preservam as proteções em ambos os lados.
    pub fn munmap(&mut self, address: u64, length: u64) -> Result<(), VmaError> {
        let start = page_floor(address);
        let length = page_ceil(length).ok_or(VmaError::InvalidRange)?;
        let end = start.checked_add(length).ok_or(VmaError::InvalidRange)?;
        if length == 0 || end > USER_TOP {
            return Err(VmaError::InvalidRange);
        }
        let index = self.regions[..self.count]
            .iter()
            .position(|entry| {
                entry.is_some_and(|region| {
                    region.kind == RegionKind::Mmap && start >= region.start && end <= region.end
                })
            })
            .ok_or(VmaError::NotMapped)?;
        let region = self.regions[index].ok_or(VmaError::NotMapped)?;
        match (start == region.start, end == region.end) {
            (true, true) => self.remove(index),
            (true, false) => {
                self.regions[index] = Some(Region {
                    start: end,
                    ..region
                })
            }
            (false, true) => {
                self.regions[index] = Some(Region {
                    end: start,
                    ..region
                })
            }
            (false, false) => {
                if self.count == N {
                    return Err(VmaError::Capacity);
                }
                self.regions[index] = Some(Region {
                    end: start,
                    ..region
                });
                self.insert(Region {
                    start: end,
                    ..region
                })?;
            }
        }
        self.mmap_cursor = self.mmap_cursor.min(start);
        Ok(())
    }

    /// Retorna o break existente em pedidos inválidos, como Linux; crescimento
    /// não pode ultrapassar limite nem colidir com outra VMA.
    pub fn brk(&mut self, requested: u64) -> u64 {
        if requested == 0 {
            return self.heap_end;
        }
        if requested < self.heap_start || requested > self.heap_limit || requested >= USER_TOP {
            return self.heap_end;
        }
        let old_page_end = page_ceil(self.heap_end).unwrap_or(self.heap_end);
        let new_page_end = page_ceil(requested).unwrap_or(requested);
        if new_page_end > old_page_end
            && !self.range_free(old_page_end, new_page_end - old_page_end)
        {
            return self.heap_end;
        }
        if self.heap_start == self.heap_end && requested > self.heap_start {
            if self.count == N {
                return self.heap_end;
            }
            self.regions[self.count] = Some(Region {
                start: self.heap_start,
                end: new_page_end,
                kind: RegionKind::Heap,
                protection: 0b011,
            });
            self.count += 1;
        } else if let Some(region) = self.regions[..self.count]
            .iter_mut()
            .flatten()
            .find(|region| region.kind == RegionKind::Heap)
        {
            if requested == self.heap_start {
                let index = self.regions[..self.count]
                    .iter()
                    .position(|entry| entry.is_some_and(|r| r.kind == RegionKind::Heap));
                if let Some(index) = index {
                    self.remove(index);
                }
            } else {
                region.end = new_page_end;
            }
        }
        self.heap_end = requested;
        requested
    }

    fn range_free(&self, start: u64, length: u64) -> bool {
        let Some(end) = start.checked_add(length) else {
            return false;
        };
        start >= PAGE_SIZE
            && end <= USER_TOP
            && self
                .regions()
                .all(|region| end <= region.start || start >= region.end)
    }

    fn insert(&mut self, region: Region) -> Result<(), VmaError> {
        if !region.start.is_multiple_of(PAGE_SIZE)
            || region.end <= region.start
            || region.end > USER_TOP
        {
            return Err(VmaError::InvalidRange);
        }
        if !self.range_free(region.start, region.end - region.start) {
            return Err(VmaError::Overlap);
        }
        if self.count == N {
            return Err(VmaError::Capacity);
        }
        let position = self.regions[..self.count]
            .iter()
            .position(|entry| entry.is_some_and(|current| current.start > region.start))
            .unwrap_or(self.count);
        self.regions.copy_within(position..self.count, position + 1);
        self.regions[position] = Some(region);
        self.count += 1;
        Ok(())
    }

    fn remove(&mut self, index: usize) {
        self.regions.copy_within(index + 1..self.count, index);
        self.count -= 1;
        self.regions[self.count] = None;
    }
}

const fn page_floor(value: u64) -> u64 {
    value & !(PAGE_SIZE - 1)
}
fn page_ceil(value: u64) -> Option<u64> {
    value.checked_add(PAGE_SIZE - 1).map(page_floor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    struct FakeTables {
        entries: BTreeMap<(u64, usize), u64>,
        allocated: BTreeSet<u64>,
        next: u64,
    }

    impl crate::memory::PageTableAccess for FakeTables {
        fn read_entry(&self, table: u64, index: usize) -> Result<u64, crate::memory::FrameError> {
            Ok(self.entries.get(&(table, index)).copied().unwrap_or(0))
        }

        fn write_entry(
            &mut self,
            table: u64,
            index: usize,
            value: u64,
        ) -> Result<(), crate::memory::FrameError> {
            self.entries.insert((table, index), value);
            Ok(())
        }

        fn allocate_table(&mut self) -> Result<u64, crate::memory::FrameError> {
            let frame = self.next;
            self.next += PAGE_SIZE;
            self.allocated.insert(frame);
            Ok(frame)
        }

        fn release_table(&mut self, physical: u64) -> Result<(), crate::memory::FrameError> {
            self.allocated
                .remove(&physical)
                .then_some(())
                .ok_or(crate::memory::FrameError::NotAllocated)
        }

        fn invalidate_page(&mut self, _virtual_address: u64) {}
    }

    fn process_vm<const P: usize>() -> ProcessVm<FakeTables, P, 8> {
        let tables = FakeTables {
            entries: BTreeMap::new(),
            allocated: BTreeSet::new(),
            next: 0x20_0000,
        };
        let space = crate::memory::OwnedAddressSpace::new(tables, 0x10_0000).unwrap();
        ProcessVm::new(space, USER_HEAP_BASE, USER_HEAP_BASE + PAGE_SIZE * 32)
    }

    #[test]
    fn mmap_is_aligned_and_first_fit_reuses_unmapped_hole() {
        let mut map = VmaMap::<8>::new(USER_HEAP_BASE, USER_HEAP_BASE + 0x10000);
        let first = map.mmap(0, 1, 0b011).unwrap();
        let second = map.mmap(0, PAGE_SIZE, 0b001).unwrap();
        assert_eq!(first % PAGE_SIZE, 0);
        map.munmap(first, PAGE_SIZE).unwrap();
        assert_eq!(map.mmap(0, PAGE_SIZE, 0b011).unwrap(), first);
        assert_ne!(first, second);
    }

    #[test]
    fn munmap_splits_regions_and_rejects_non_mmap_holes() {
        let mut map = VmaMap::<8>::new(USER_HEAP_BASE, USER_HEAP_BASE + 0x10000);
        let base = map.mmap(0, PAGE_SIZE * 3, 0b011).unwrap();
        map.munmap(base + PAGE_SIZE, PAGE_SIZE).unwrap();
        let pieces: std::vec::Vec<_> = map.regions().collect();
        assert_eq!(pieces.len(), 2);
        assert_eq!((pieces[0].start, pieces[0].end), (base, base + PAGE_SIZE));
        assert_eq!(
            (pieces[1].start, pieces[1].end),
            (base + PAGE_SIZE * 2, base + PAGE_SIZE * 3)
        );
        assert_eq!(
            map.munmap(base + PAGE_SIZE, PAGE_SIZE),
            Err(VmaError::NotMapped)
        );
    }

    #[test]
    fn brk_grows_shrinks_and_refuses_mmap_collision() {
        let mut map = VmaMap::<8>::new(USER_HEAP_BASE, USER_HEAP_BASE + PAGE_SIZE * 8);
        assert_eq!(map.brk(USER_HEAP_BASE + 17), USER_HEAP_BASE + 17);
        assert_eq!(
            map.brk(USER_HEAP_BASE + PAGE_SIZE * 2),
            USER_HEAP_BASE + PAGE_SIZE * 2
        );
        assert_eq!(map.brk(USER_HEAP_BASE), USER_HEAP_BASE);
        assert_eq!(map.brk(USER_HEAP_BASE - 1), USER_HEAP_BASE);

        let mut colliding = VmaMap::<8>::new(MMAP_BASE + PAGE_SIZE, MMAP_BASE + PAGE_SIZE * 8);
        colliding
            .mmap(MMAP_BASE + PAGE_SIZE * 2, PAGE_SIZE, 0b011)
            .unwrap();
        assert_eq!(
            colliding.brk(MMAP_BASE + PAGE_SIZE * 4),
            MMAP_BASE + PAGE_SIZE
        );
    }

    #[test]
    fn mmap_rejects_writable_executable_and_overflow() {
        let mut map = VmaMap::<2>::new(USER_HEAP_BASE, USER_HEAP_BASE + PAGE_SIZE * 8);
        assert_eq!(map.mmap(0, 32, 0b111), Err(VmaError::InvalidRange));
        assert_eq!(map.mmap(0, u64::MAX, 0b011), Err(VmaError::InvalidRange));
    }

    #[test]
    fn process_vm_maps_and_unmaps_owned_user_pages_with_vma_state() {
        let mut vm = process_vm::<8>();
        let base = vm.mmap_anonymous(0, PAGE_SIZE * 2, 0b011).unwrap();
        assert_eq!(
            vm.mapping(base).unwrap().flags & crate::memory::PAGE_FLAG_USER,
            4
        );
        assert_ne!(
            vm.mapping(base).unwrap().flags & crate::memory::PAGE_FLAG_WRITABLE,
            0
        );
        vm.munmap(base, PAGE_SIZE * 2).unwrap();
        assert_eq!(vm.mapping(base), Err(crate::memory::FrameError::NotMapped));
        assert_eq!(vm.mmap_anonymous(base, PAGE_SIZE, 0b001), Ok(base));
    }

    #[test]
    fn process_vm_accounts_owned_leaf_and_private_table_frames() {
        let mut vm = process_vm::<8>();
        let address = vm.mmap_anonymous(0, PAGE_SIZE, 0b011).unwrap();
        assert_eq!(vm.owned_user_pages(), 1);
        assert_eq!(vm.owned_page_table_pages().unwrap(), 4);
        let usage = vm.memory_usage().unwrap();
        assert_eq!(usage.total_owned_pages, 5);
        assert_eq!(usage.maximum_owned_pages, 33);
        vm.munmap(address, PAGE_SIZE).unwrap();
        assert_eq!(vm.owned_user_pages(), 0);
        assert_eq!(vm.owned_page_table_pages().unwrap(), 1);
        assert_eq!(vm.memory_usage().unwrap().total_owned_pages, 1);
    }

    #[test]
    fn process_vm_mprotect_splits_vma_and_rejects_wx_and_none() {
        let mut vm = process_vm::<8>();
        let base = vm.mmap_anonymous(0, PAGE_SIZE * 3, 0b011).unwrap();
        vm.mprotect(base + PAGE_SIZE, PAGE_SIZE, 0b001).unwrap();
        assert_ne!(
            vm.mapping(base).unwrap().flags & crate::memory::PAGE_FLAG_WRITABLE,
            0
        );
        assert_eq!(
            vm.mapping(base + PAGE_SIZE).unwrap().flags & crate::memory::PAGE_FLAG_WRITABLE,
            0
        );
        assert_ne!(
            vm.mapping(base + PAGE_SIZE * 2).unwrap().flags & crate::memory::PAGE_FLAG_WRITABLE,
            0
        );
        assert_eq!(
            vm.mprotect(base, PAGE_SIZE, 0b110),
            Err(ProcessVmError::PermissionDenied)
        );
        assert_eq!(
            vm.mprotect(base, PAGE_SIZE, 0),
            Err(ProcessVmError::UnsupportedMapping)
        );
        vm.munmap(base, PAGE_SIZE).unwrap();
        vm.munmap(base + PAGE_SIZE, PAGE_SIZE).unwrap();
        vm.munmap(base + PAGE_SIZE * 2, PAGE_SIZE).unwrap();
    }

    #[test]
    fn process_vm_brk_owns_pages_and_respects_vma_collisions() {
        let mut vm = process_vm::<8>();
        assert_eq!(vm.brk(USER_HEAP_BASE + 1), Ok(USER_HEAP_BASE + 1));
        assert!(vm.mapping(USER_HEAP_BASE).is_ok());
        let mmap = vm
            .mmap_anonymous(USER_HEAP_BASE + PAGE_SIZE * 4, PAGE_SIZE, 0b011)
            .unwrap();
        assert_eq!(vm.brk(mmap + PAGE_SIZE), Ok(USER_HEAP_BASE + 1));
        assert_eq!(vm.brk(USER_HEAP_BASE), Ok(USER_HEAP_BASE));
        assert_eq!(
            vm.mapping(USER_HEAP_BASE),
            Err(crate::memory::FrameError::NotMapped)
        );
    }

    #[test]
    fn process_vm_linux_mapping_flags_are_checked_before_allocating() {
        let mut vm = process_vm::<8>();
        assert_eq!(
            vm.mmap_linux(0, PAGE_SIZE, 0b011, 0x22, u64::MAX, 0),
            Ok(MMAP_BASE)
        );
        assert_eq!(
            vm.mmap_linux(0, PAGE_SIZE, 0b011, 0x21, u64::MAX, 0),
            Err(ProcessVmError::UnsupportedMapping)
        );
        assert_eq!(
            vm.mmap_linux(0, PAGE_SIZE, 0b111, 0x22, u64::MAX, 0),
            Err(ProcessVmError::Vma(VmaError::InvalidRange))
        );
        assert_eq!(
            vm.mmap_linux(0, PAGE_SIZE, 0, 0x22, u64::MAX, 0),
            Err(ProcessVmError::UnsupportedMapping)
        );
    }
}

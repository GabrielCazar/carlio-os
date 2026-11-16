//! Heap de região para a futura alocação dinâmica do kernel.
//!
//! O chamador fornece uma região mapeada e exclusiva. O serviço global
//! sincroniza o acesso externo com SpinLock.

use core::alloc::Layout;
use core::mem::{align_of, size_of};
use core::ptr::{self, NonNull};

const MAGIC: usize = 0xCA71_1005;

#[repr(C, align(16))]
struct Block {
    magic: usize,
    span: usize,
    used: usize,
    user_offset: usize,
}

/// Allocator first-fit sobre um buffer mapeado. Métodos mutáveis requerem
/// exclusividade; os ponteiros retornados permanecem válidos até `deallocate`.
pub struct KernelHeap {
    start: NonNull<u8>,
    len: usize,
}

/// Set of independently mapped heap regions. New regions can be attached at
/// runtime (for example, when the kernel maps another physical frame) without
/// requiring physical contiguity between arenas.
pub struct KernelHeapSet<const REGIONS: usize> {
    regions: [Option<KernelHeap>; REGIONS],
    region_count: usize,
}

impl<const REGIONS: usize> KernelHeapSet<REGIONS> {
    pub const fn new() -> Self {
        Self {
            regions: [const { None }; REGIONS],
            region_count: 0,
        }
    }

    /// Adds a separately mapped, exclusive arena.
    ///
    /// # Safety
    /// `memory..memory+len` must be a valid writable region, exclusively owned
    /// by this heap set, and remain mapped until all allocations are released.
    pub unsafe fn add_region(&mut self, memory: NonNull<u8>, len: usize) -> Result<(), HeapError> {
        let start = memory.as_ptr() as usize;
        let end = start.checked_add(len).ok_or(HeapError::InvalidRegion)?;
        for heap in self.regions[..self.region_count].iter().flatten() {
            let existing_start = heap.start.as_ptr() as usize;
            let existing_end = existing_start
                .checked_add(heap.len)
                .ok_or(HeapError::Corrupt)?;
            if start < existing_end && existing_start < end {
                return Err(HeapError::OverlappingRegion);
            }
        }
        if self.region_count == REGIONS {
            return Err(HeapError::NoRegionSlots);
        }
        let heap = unsafe { KernelHeap::new(memory, len) }.ok_or(HeapError::InvalidRegion)?;
        self.regions[self.region_count] = Some(heap);
        self.region_count += 1;
        Ok(())
    }

    pub fn allocate(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        self.regions[..self.region_count]
            .iter_mut()
            .flatten()
            .find_map(|heap| heap.allocate(layout))
    }

    /// # Safety
    /// `pointer` must be a live allocation returned by this heap set and must
    /// not be used after this call succeeds.
    pub unsafe fn deallocate(&mut self, pointer: NonNull<u8>) -> Result<(), HeapError> {
        let address = pointer.as_ptr() as usize;
        let heap = self.regions[..self.region_count]
            .iter_mut()
            .flatten()
            .find(|heap| {
                let start = heap.start.as_ptr() as usize;
                address >= start && address < start.saturating_add(heap.len)
            })
            .ok_or(HeapError::ForeignPointer)?;
        unsafe { heap.deallocate(pointer) }
    }

    pub const fn region_count(&self) -> usize {
        self.region_count
    }

    pub fn capacity(&self) -> usize {
        self.regions[..self.region_count]
            .iter()
            .flatten()
            .map(KernelHeap::capacity)
            .sum()
    }
}

impl<const REGIONS: usize> Default for KernelHeapSet<REGIONS> {
    fn default() -> Self {
        Self::new()
    }
}

// A região é exclusiva do heap e acessada de forma serializada pelo lock.
unsafe impl Send for KernelHeap {}

impl KernelHeap {
    /// Inicializa uma região sem objetos vivos.
    ///
    /// # Safety
    /// `memory..memory+len` deve ser válida, gravável e exclusiva, e permanecer
    /// mapeada e reservada enquanto o heap existir.
    pub unsafe fn new(memory: NonNull<u8>, len: usize) -> Option<Self> {
        let alignment = align_of::<Block>();
        let address = memory.as_ptr() as usize;
        let aligned = align_up(address, alignment)?;
        let skipped = aligned.checked_sub(address)?;
        let usable = len.checked_sub(skipped)? & !(alignment - 1);
        if usable < size_of::<Block>() + size_of::<usize>() + 1 {
            return None;
        }
        let start = NonNull::new(aligned as *mut u8)?;
        ptr::write(
            start.as_ptr().cast::<Block>(),
            Block {
                magic: MAGIC,
                span: usable,
                used: 0,
                user_offset: 0,
            },
        );
        Some(Self { start, len: usable })
    }

    /// Reserva um bloco alinhado; retorna `None` em exaustão ou layout inválido.
    pub fn allocate(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        let minimum = layout.size().max(1);
        let mut offset = 0usize;
        while offset.checked_add(size_of::<Block>())? <= self.len {
            let block_ptr = unsafe { self.start.as_ptr().add(offset).cast::<Block>() };
            let block = unsafe { &mut *block_ptr };
            if block.magic != MAGIC
                || block.span < size_of::<Block>()
                || block.used > 1
                || offset.checked_add(block.span)? > self.len
            {
                return None;
            }
            if block.used == 0 {
                let payload_floor = (block_ptr as usize)
                    .checked_add(size_of::<Block>())?
                    .checked_add(size_of::<usize>())?;
                let payload = align_up(payload_floor, layout.align().max(align_of::<usize>()))?;
                let end = payload.checked_add(minimum)?;
                let block_end = (block_ptr as usize).checked_add(block.span)?;
                if end <= block_end {
                    let needed =
                        align_up(end.checked_sub(block_ptr as usize)?, align_of::<Block>())?;
                    let remainder = block.span.checked_sub(needed)?;
                    if remainder > size_of::<Block>() + size_of::<usize>() {
                        let next = unsafe { block_ptr.cast::<u8>().add(needed).cast::<Block>() };
                        unsafe {
                            ptr::write(
                                next,
                                Block {
                                    magic: MAGIC,
                                    span: remainder,
                                    used: 0,
                                    user_offset: 0,
                                },
                            )
                        };
                        block.span = needed;
                    }
                    block.used = 1;
                    let user = payload as *mut u8;
                    block.user_offset = payload.checked_sub(block_ptr as usize)?;
                    return NonNull::new(user);
                }
            }
            offset = offset.checked_add(block.span)?;
        }
        None
    }

    /// Libera um ponteiro deste heap e une blocos livres adjacentes.
    /// Retorna erro para ponteiro estrangeiro, corrompido ou já liberado.
    ///
    /// # Safety
    /// `pointer` deve ser um bloco ainda alocado por este heap e não pode ser
    /// usado novamente depois que esta função o libera.
    pub unsafe fn deallocate(&mut self, pointer: NonNull<u8>) -> Result<(), HeapError> {
        let address = pointer.as_ptr() as usize;
        let base = self.start.as_ptr() as usize;
        let end = base.checked_add(self.len).ok_or(HeapError::Corrupt)?;
        if address < base + size_of::<Block>() + size_of::<usize>() || address >= end {
            return Err(HeapError::ForeignPointer);
        }
        // Nunca derive o cabeçalho de bytes adjacentes ao ponteiro fornecido:
        // ponteiros interiores e payload adulterado não podem escolher o que
        // será interpretado como metadado. Caminhe a cadeia validando spans.
        let mut offset = 0usize;
        let block = loop {
            let block_address = base.checked_add(offset).ok_or(HeapError::Corrupt)?;
            if block_address
                .checked_add(size_of::<Block>())
                .ok_or(HeapError::Corrupt)?
                > end
            {
                return Err(HeapError::Corrupt);
            }
            let candidate = &mut *(block_address as *mut Block);
            if candidate.magic != MAGIC
                || candidate.span < size_of::<Block>()
                || !candidate.span.is_multiple_of(align_of::<Block>())
                || candidate.used > 1
                || block_address
                    .checked_add(candidate.span)
                    .filter(|v| *v <= end)
                    .is_none()
            {
                return Err(HeapError::Corrupt);
            }
            if candidate.used == 1
                && candidate.user_offset >= size_of::<Block>()
                && block_address.checked_add(candidate.user_offset) == Some(address)
            {
                break candidate;
            }
            if block_address == address {
                return Err(HeapError::ForeignPointer);
            }
            offset = offset
                .checked_add(candidate.span)
                .ok_or(HeapError::Corrupt)?;
            if offset >= self.len {
                return Err(HeapError::DoubleFreeOrCorrupt);
            }
        };
        if block.used != 1 {
            return Err(HeapError::DoubleFreeOrCorrupt);
        }
        block.used = 0;
        block.user_offset = 0;
        self.coalesce()?;
        Ok(())
    }

    fn coalesce(&mut self) -> Result<(), HeapError> {
        let mut offset = 0usize;
        while offset < self.len {
            let block_ptr = unsafe { self.start.as_ptr().add(offset).cast::<Block>() };
            let block = unsafe { &mut *block_ptr };
            if block.magic != MAGIC
                || block.span < size_of::<Block>()
                || block.used > 1
                || offset
                    .checked_add(block.span)
                    .filter(|end| *end <= self.len)
                    .is_none()
            {
                return Err(HeapError::Corrupt);
            }
            let next_offset = offset + block.span;
            if block.used == 0 && next_offset < self.len {
                let next = unsafe { self.start.as_ptr().add(next_offset).cast::<Block>() };
                let next_ref = unsafe { &*next };
                if next_ref.magic != MAGIC
                    || next_ref.span < size_of::<Block>()
                    || next_ref.used > 1
                {
                    return Err(HeapError::Corrupt);
                }
                if next_ref.used == 0 {
                    block.span = block
                        .span
                        .checked_add(next_ref.span)
                        .ok_or(HeapError::Corrupt)?;
                    continue;
                }
            }
            offset = next_offset;
        }
        Ok(())
    }

    pub const fn capacity(&self) -> usize {
        self.len
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeapError {
    ForeignPointer,
    DoubleFreeOrCorrupt,
    Corrupt,
    InvalidRegion,
    OverlappingRegion,
    NoRegionSlots,
}

fn align_up(value: usize, alignment: usize) -> Option<usize> {
    debug_assert!(alignment.is_power_of_two());
    value
        .checked_add(alignment - 1)
        .map(|v| v & !(alignment - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[repr(align(4096))]
    struct Arena([u8; 32 * 1024]);

    fn heap(arena: &mut Arena) -> KernelHeap {
        unsafe {
            KernelHeap::new(NonNull::new(arena.0.as_mut_ptr()).unwrap(), arena.0.len()).unwrap()
        }
    }

    #[test]
    fn honors_size_and_large_alignment_then_reuses_coalesced_space() {
        let mut arena = Arena([0; 32 * 1024]);
        let mut heap = heap(&mut arena);
        let a = heap
            .allocate(Layout::from_size_align(300, 256).unwrap())
            .unwrap();
        let b = heap
            .allocate(Layout::from_size_align(700, 4096).unwrap())
            .unwrap();
        assert_eq!(a.as_ptr() as usize % 256, 0);
        assert_eq!(b.as_ptr() as usize % 4096, 0);
        unsafe {
            a.as_ptr().write_bytes(0xA5, 300);
            b.as_ptr().write_bytes(0x5A, 700);
        }
        unsafe {
            heap.deallocate(a).unwrap();
            heap.deallocate(b).unwrap();
        }
        let whole = heap
            .allocate(Layout::from_size_align(20_000, 16).unwrap())
            .unwrap();
        assert_ne!(whole.as_ptr() as usize, 0);
    }

    #[test]
    fn rejects_foreign_and_double_free() {
        let mut arena = Arena([0; 32 * 1024]);
        let mut heap = heap(&mut arena);
        let value = heap
            .allocate(Layout::from_size_align(32, 8).unwrap())
            .unwrap();
        let foreign = NonNull::<u8>::dangling();
        assert_eq!(
            unsafe { heap.deallocate(foreign) },
            Err(HeapError::ForeignPointer)
        );
        unsafe {
            heap.deallocate(value).unwrap();
        }
        assert!(matches!(
            unsafe { heap.deallocate(value) },
            Err(HeapError::DoubleFreeOrCorrupt | HeapError::Corrupt)
        ));
    }

    #[test]
    fn rejects_too_small_region_and_exhausts_cleanly() {
        let mut tiny = [0u8; 16];
        assert!(
            unsafe { KernelHeap::new(NonNull::new(tiny.as_mut_ptr()).unwrap(), tiny.len()) }
                .is_none()
        );
        let mut arena = Arena([0; 32 * 1024]);
        let mut heap = heap(&mut arena);
        assert!(heap
            .allocate(Layout::from_size_align(isize::MAX as usize - 7, 8).unwrap())
            .is_none());
        assert!(heap
            .allocate(Layout::from_size_align(isize::MAX as usize - 4095, 4096).unwrap())
            .is_none());
    }

    #[test]
    fn heap_set_expands_into_new_regions_and_routes_deallocation() {
        let mut first = Arena([0; 32 * 1024]);
        let mut second = Arena([0; 32 * 1024]);
        let mut heaps = KernelHeapSet::<2>::new();
        unsafe {
            heaps
                .add_region(NonNull::new(first.0.as_mut_ptr()).unwrap(), first.0.len())
                .unwrap();
        }
        let first_allocation = heaps
            .allocate(Layout::from_size_align(300, 16).unwrap())
            .unwrap();
        unsafe {
            heaps
                .add_region(NonNull::new(second.0.as_mut_ptr()).unwrap(), second.0.len())
                .unwrap();
        }
        let second_allocation = heaps
            .allocate(Layout::from_size_align(300, 16).unwrap())
            .unwrap();
        assert_eq!(heaps.region_count(), 2);
        unsafe {
            heaps.deallocate(first_allocation).unwrap();
            heaps.deallocate(second_allocation).unwrap();
        }
    }

    #[test]
    fn heap_set_rejects_overlapping_and_excess_regions() {
        let mut arena = Arena([0; 32 * 1024]);
        let mut heaps = KernelHeapSet::<1>::new();
        let base = NonNull::new(arena.0.as_mut_ptr()).unwrap();
        unsafe {
            heaps.add_region(base, arena.0.len()).unwrap();
            assert_eq!(
                heaps.add_region(NonNull::new(base.as_ptr().add(4096)).unwrap(), 4096),
                Err(HeapError::OverlappingRegion)
            );
            assert_eq!(
                heaps.add_region(NonNull::new(0x1000usize as *mut u8).unwrap(), 4096),
                Err(HeapError::NoRegionSlots)
            );
        }
    }
}

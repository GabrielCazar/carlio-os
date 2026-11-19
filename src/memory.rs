//! Conversões de endereços fornecidas pelo protocolo Limine.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KernelAddress {
    pub physical_base: u64,
    pub virtual_base: u64,
}

impl KernelAddress {
    /// Converte somente endereços dentro do segmento carregado do kernel.
    /// O chamador ainda precisa garantir que o intervalo completo do buffer
    /// pertence ao kernel e está contíguo para DMA.
    pub fn virtual_to_physical(self, address: u64) -> Option<u64> {
        let offset = address.checked_sub(self.virtual_base)?;
        self.physical_base.checked_add(offset)
    }
}

pub fn hhdm_address(offset: u64, physical: u64) -> Option<u64> {
    offset.checked_add(physical)
}

/// Alocador de frames físicos contíguos dentro de uma região previamente
/// validada pelo mapa Limine. A bitmap é fornecida pelo chamador para não
/// depender de heap durante a inicialização.
pub struct FrameAllocator<const WORDS: usize> {
    base: u64,
    frame_count: usize,
    bitmap: [u64; WORDS],
    allocated: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    UnalignedBase,
    CapacityExceeded,
    InvalidFrame,
    AlreadyFree,
    InvalidRegion,
    NotAllocated,
    InvalidVirtualAddress,
    InvalidPhysicalAddress,
    InvalidPageFlags,
    AlreadyMapped,
    NotMapped,
    HugePageConflict,
    TableAllocationFailed,
    CorruptTable,
}

pub const PAGE_TABLE_ENTRIES: usize = 512;
const ENTRY_ADDRESS_MASK: u64 = 0x000f_ffff_ffff_f000;
const ENTRY_PRESENT: u64 = 1;
const ENTRY_HUGE: u64 = 1 << 7;
const ENTRY_USER: u64 = 1 << 2;
const ENTRY_WRITABLE: u64 = 1 << 1;
pub const PAGE_FLAG_NO_EXECUTE: u64 = 1 << 63;
pub const PAGE_FLAG_USER: u64 = ENTRY_USER;
pub const PAGE_FLAG_WRITABLE: u64 = ENTRY_WRITABLE;
const ENTRY_ALLOWED_FLAGS: u64 = 0x8000_0000_0000_01ff;

/// Backend for a four-level x86_64 page-table tree. `allocate_table` must
/// return a zeroed, owned 4 KiB frame. Entries are indexed from 0 through 511.
pub trait PageTableAccess {
    fn read_entry(&self, table: u64, index: usize) -> Result<u64, FrameError>;
    fn write_entry(&mut self, table: u64, index: usize, value: u64) -> Result<(), FrameError>;
    fn allocate_table(&mut self) -> Result<u64, FrameError>;
    fn release_table(&mut self, physical: u64) -> Result<(), FrameError>;
    /// Aloca um frame exclusivo para dados de usuário. Implementações que
    /// distinguem páginas de tabela e páginas comuns podem sobrescrever estes
    /// métodos; o padrão mantém compatibilidade com backends simples.
    fn allocate_user_frame(&mut self) -> Result<u64, FrameError> {
        self.allocate_table()
    }
    fn release_user_frame(&mut self, physical: u64) -> Result<(), FrameError> {
        self.release_table(physical)
    }
    fn invalidate_page(&mut self, virtual_address: u64);
}

/// Edits an existing PML4 tree. The root frame remains owned by the caller;
/// intermediate tables created by this manager are reclaimed when empty.
pub struct PageTableManager<A> {
    access: A,
    root: u64,
}

/// Implementação arquitetural que deve enviar um pedido de invalidação a
/// todas as CPUs que possam usar `root` e aguardar os acknowledgements antes
/// de retornar. `invlpg` local sozinho não satisfaz esse contrato.
pub trait TlbShootdown: Send + Sync {
    fn invalidate_all(&self, root: u64, virtual_address: u64);
}

/// Serializa alterações da mesma árvore de páginas entre CPUs. A sincronização
/// remota do TLB é obrigatória e fica explícita no backend `TlbShootdown`.
pub struct SmpPageTableManager<A, S> {
    tables: crate::sync::IrqSpinLock<PageTableManager<A>>,
    shootdown: S,
}

impl<A: PageTableAccess, S: TlbShootdown> SmpPageTableManager<A, S> {
    pub fn new(access: A, root: u64, shootdown: S) -> Result<Self, FrameError> {
        Ok(Self {
            tables: crate::sync::IrqSpinLock::new(PageTableManager::new(access, root)?),
            shootdown,
        })
    }

    pub fn root(&self) -> u64 {
        self.tables.lock().root()
    }

    pub fn map(&self, virtual_address: u64, physical: u64, flags: u64) -> Result<(), FrameError> {
        let root = {
            let mut tables = self.tables.lock();
            tables.map(virtual_address, physical, flags)?;
            tables.root()
        };
        self.shootdown.invalidate_all(root, virtual_address);
        Ok(())
    }

    pub fn unmap(&self, virtual_address: u64) -> Result<u64, FrameError> {
        let (root, physical) = {
            let mut tables = self.tables.lock();
            let physical = tables.unmap(virtual_address)?;
            (tables.root(), physical)
        };
        self.shootdown.invalidate_all(root, virtual_address);
        Ok(physical)
    }

    /// Altera permissões e aguarda o shootdown SMP antes de retornar. O frame
    /// permanece sob ownership do chamador durante toda a operação.
    pub fn protect(&self, virtual_address: u64, flags: u64) -> Result<(), FrameError> {
        let root = {
            let mut tables = self.tables.lock();
            tables.protect(virtual_address, flags)?;
            tables.root()
        };
        self.shootdown.invalidate_all(root, virtual_address);
        Ok(())
    }

    pub fn mapping(&self, virtual_address: u64) -> Result<PageMapping, FrameError> {
        self.tables.lock().mapping(virtual_address)
    }
}

/// CR3 independente que compartilha somente o half superior do kernel.
/// Frames das folhas user continuam pertencendo ao chamador. O processo deve
/// chamar `try_destroy` ao sair para liberar suas tabelas de página.
pub struct AddressSpace<A, const MAPPINGS: usize> {
    tables: PageTableManager<A>,
    root: u64,
    user_mappings: [u64; MAPPINGS],
    user_mapping_count: usize,
}

/// Espaço de usuário que também possui os frames das folhas mapeadas por ele.
/// O dono não pode liberar um frame enquanto ele estiver mapeado.
pub struct OwnedAddressSpace<A, const PAGES: usize> {
    space: AddressSpace<A, PAGES>,
    frames: [(u64, u64); PAGES], // (VA, PA)
    frame_count: usize,
}

impl<A: PageTableAccess, const PAGES: usize> OwnedAddressSpace<A, PAGES> {
    pub fn new(access: A, kernel_root: u64) -> Result<Self, FrameError> {
        Ok(Self {
            space: AddressSpace::new(access, kernel_root)?,
            frames: [(0, 0); PAGES],
            frame_count: 0,
        })
    }

    pub const fn root(&self) -> u64 {
        self.space.root()
    }

    /// Aloca, zera e mapeia uma página de usuário. Em falha de mapeamento o
    /// frame recém-alocado é devolvido imediatamente ao backend.
    pub fn map_zeroed(&mut self, virtual_address: u64, flags: u64) -> Result<u64, FrameError> {
        if self.frames[..self.frame_count]
            .iter()
            .any(|(address, _)| *address == virtual_address)
        {
            return Err(FrameError::AlreadyMapped);
        }
        if self.frame_count == PAGES {
            return Err(FrameError::CapacityExceeded);
        }
        let physical = self.space.tables.access.allocate_user_frame()?;
        if physical % PAGE_SIZE != 0 || physical & !ENTRY_ADDRESS_MASK != 0 {
            let _ = self.space.tables.access.release_user_frame(physical);
            return Err(FrameError::InvalidPhysicalAddress);
        }
        if let Err(error) = self.map_owned_frame(virtual_address, physical, flags) {
            let _ = self.space.tables.access.release_user_frame(physical);
            return Err(error);
        }
        Ok(physical)
    }

    /// Transfere para o espaço ownership de um frame já alocado pelo chamador.
    pub fn map_owned_frame(
        &mut self,
        virtual_address: u64,
        physical: u64,
        flags: u64,
    ) -> Result<(), FrameError> {
        if self.frame_count == PAGES {
            return Err(FrameError::CapacityExceeded);
        }
        if self.frames[..self.frame_count]
            .iter()
            .any(|(address, _)| *address == virtual_address)
        {
            return Err(FrameError::AlreadyMapped);
        }
        self.space.map_user(virtual_address, physical, flags)?;
        self.frames[self.frame_count] = (virtual_address, physical);
        self.frame_count += 1;
        Ok(())
    }

    pub fn mapping(&self, virtual_address: u64) -> Result<PageMapping, FrameError> {
        self.space.mapping(virtual_address)
    }

    /// Ajusta permissões de uma página própria mantendo seu frame sob ownership.
    pub fn protect(&mut self, virtual_address: u64, flags: u64) -> Result<(), FrameError> {
        if !self.frames[..self.frame_count]
            .iter()
            .any(|(va, _)| *va == virtual_address)
        {
            return Err(FrameError::NotMapped);
        }
        self.space.protect_user(virtual_address, flags)
    }

    /// Desmapeia e devolve o frame de dados ao allocator.
    pub fn unmap(&mut self, virtual_address: u64) -> Result<(), FrameError> {
        let slot = self.frames[..self.frame_count]
            .iter()
            .position(|(address, _)| *address == virtual_address)
            .ok_or(FrameError::NotMapped)?;
        let mapping = self.space.mapping(virtual_address)?;
        let physical = self.space.unmap_user(virtual_address)?;
        if let Err(error) = self.space.tables.access.release_user_frame(physical) {
            // A folha já foi removida; restaura o mapeamento antes de devolver
            // o erro para manter ownership e visibilidade consistentes.
            let restore = self
                .space
                .map_user(virtual_address, physical, mapping.flags);
            return Err(restore.err().unwrap_or(error));
        }
        self.frames.copy_within(slot + 1..self.frame_count, slot);
        self.frame_count -= 1;
        self.frames[self.frame_count] = (0, 0);
        Ok(())
    }

    /// Libera folhas, tabelas user e a raiz. Em falha retorna o objeto para
    /// retry, preservando os frames ainda pertencentes ao espaço.
    pub fn try_destroy(mut self) -> Result<A, (Self, FrameError)> {
        while self.frame_count != 0 {
            let address = self.frames[self.frame_count - 1].0;
            if let Err(error) = self.unmap(address) {
                return Err((self, error));
            }
        }
        let space = self.space;
        match space.try_destroy() {
            Ok(access) => Ok(access),
            Err((space, error)) => {
                self.space = space;
                Err((self, error))
            }
        }
    }
}

impl<A: PageTableAccess, const MAPPINGS: usize> AddressSpace<A, MAPPINGS> {
    /// Cria novo PML4 vazio e copia as entradas PML4 256..512 do kernel.
    /// Essas entradas são compartilhadas e nunca são liberadas pelo processo.
    pub fn new(mut access: A, kernel_root: u64) -> Result<Self, FrameError> {
        if !kernel_root.is_multiple_of(PAGE_SIZE) || kernel_root & !ENTRY_ADDRESS_MASK != 0 {
            return Err(FrameError::InvalidPhysicalAddress);
        }
        let root = access.allocate_table()?;
        if !root.is_multiple_of(PAGE_SIZE) || root & !ENTRY_ADDRESS_MASK != 0 {
            let _ = access.release_table(root);
            return Err(FrameError::InvalidPhysicalAddress);
        }
        for index in 256..PAGE_TABLE_ENTRIES {
            let entry = match access.read_entry(kernel_root, index) {
                Ok(entry) => entry,
                Err(error) => {
                    let _ = access.release_table(root);
                    return Err(error);
                }
            };
            if entry & ENTRY_PRESENT != 0 {
                // U/S é cumulativo em x86-64. Limpar o bit no PML4 privado
                // mantém todo o half superior supervisor, mesmo que a árvore
                // compartilhada tenha um nível inferior marcado como user.
                if let Err(error) = access.write_entry(root, index, entry & !ENTRY_USER) {
                    let _ = access.release_table(root);
                    return Err(error);
                }
            }
        }
        let tables = PageTableManager::new(access, root)?;
        Ok(Self {
            tables,
            root,
            user_mappings: [0; MAPPINGS],
            user_mapping_count: 0,
        })
    }

    pub const fn root(&self) -> u64 {
        self.root
    }

    /// Número de folhas user owned neste address space. O valor permite
    /// contabilização isolada por processo sem consultar tabelas compartilhadas.
    pub const fn owned_user_pages(&self) -> usize {
        self.frame_count
    }

    /// Tabelas de página privadas alcançáveis na metade user, incluindo a raiz.
    pub fn owned_user_page_table_pages(&self) -> Result<usize, FrameError> {
        self.space.tables.user_page_table_pages()
    }

    pub fn map_user(
        &mut self,
        virtual_address: u64,
        physical: u64,
        flags: u64,
    ) -> Result<(), FrameError> {
        if self.user_mapping_count == MAPPINGS {
            return Err(FrameError::CapacityExceeded);
        }
        self.tables.map_user(virtual_address, physical, flags)?;
        self.user_mappings[self.user_mapping_count] = virtual_address;
        self.user_mapping_count += 1;
        Ok(())
    }

    /// Desmapeia somente folhas registradas por este espaço; não libera o frame
    /// de dados para permitir ownership explícito pelo allocator do processo.
    pub fn unmap_user(&mut self, virtual_address: u64) -> Result<u64, FrameError> {
        let slot = self.user_mappings[..self.user_mapping_count]
            .iter()
            .position(|address| *address == virtual_address)
            .ok_or(FrameError::NotMapped)?;
        let physical = self.tables.unmap(virtual_address)?;
        self.user_mappings
            .copy_within(slot + 1..self.user_mapping_count, slot);
        self.user_mapping_count -= 1;
        self.user_mappings[self.user_mapping_count] = 0;
        Ok(physical)
    }

    pub fn mapping(&self, virtual_address: u64) -> Result<PageMapping, FrameError> {
        if !self.user_mappings[..self.user_mapping_count].contains(&virtual_address) {
            return Err(FrameError::NotMapped);
        }
        self.tables.mapping(virtual_address)
    }

    pub fn protect_user(&mut self, virtual_address: u64, flags: u64) -> Result<(), FrameError> {
        if !self.user_mappings[..self.user_mapping_count].contains(&virtual_address) {
            return Err(FrameError::NotMapped);
        }
        self.tables.protect_user(virtual_address, flags)
    }

    /// Libera tabelas user e o PML4. Se uma operação falhar, devolve o espaço
    /// ainda disponível para que o chamador trate o erro/retry sem perder o CR3.
    pub fn try_destroy(mut self) -> Result<A, (Self, FrameError)> {
        while self.user_mapping_count != 0 {
            let address = self.user_mappings[self.user_mapping_count - 1];
            if let Err(error) = self.unmap_user(address) {
                return Err((self, error));
            }
        }
        match self.tables.access.release_table(self.root) {
            Ok(()) => Ok(self.tables.into_access()),
            Err(error) => Err((self, error)),
        }
    }
}

impl<A: PageTableAccess> PageTableManager<A> {
    pub fn new(access: A, root: u64) -> Result<Self, FrameError> {
        if !root.is_multiple_of(PAGE_SIZE) || root & !ENTRY_ADDRESS_MASK != 0 {
            return Err(FrameError::InvalidPhysicalAddress);
        }
        Ok(Self { access, root })
    }

    pub const fn root(&self) -> u64 {
        self.root
    }
    pub fn into_access(self) -> A {
        self.access
    }

    pub fn map(
        &mut self,
        virtual_address: u64,
        physical: u64,
        flags: u64,
    ) -> Result<(), FrameError> {
        if flags & ENTRY_USER != 0 {
            return Err(FrameError::InvalidPageFlags);
        }
        self.map_inner(virtual_address, physical, flags, false)
    }

    /// Mapeia uma folha de usuário no half inferior canônico. O chamador deve
    /// fornecer um frame exclusivo do processo; o backend ainda não conhece
    /// proprietário dos frames. Não passe frames do kernel nesta API.
    pub fn map_user(
        &mut self,
        virtual_address: u64,
        physical: u64,
        flags: u64,
    ) -> Result<(), FrameError> {
        if virtual_address >> 47 != 0 {
            return Err(FrameError::InvalidVirtualAddress);
        }
        self.map_inner(virtual_address, physical, flags | ENTRY_USER, true)
    }

    /// Substitui as permissões de uma folha supervisor sem alterar seu frame.
    pub fn protect(&mut self, virtual_address: u64, flags: u64) -> Result<(), FrameError> {
        if flags & !ENTRY_ALLOWED_FLAGS != 0
            || flags & ENTRY_PRESENT == 0
            || flags & ENTRY_USER != 0
        {
            return Err(FrameError::InvalidPageFlags);
        }
        let indices = page_indices(virtual_address)?;
        let mut table = self.root;
        for index in indices.iter().take(3) {
            let entry = self.access.read_entry(table, *index)?;
            if entry & ENTRY_PRESENT == 0 {
                return Err(FrameError::NotMapped);
            }
            if entry & ENTRY_HUGE != 0 {
                return Err(FrameError::HugePageConflict);
            }
            table = entry & ENTRY_ADDRESS_MASK;
        }
        let leaf_index = indices[3];
        let entry = self.access.read_entry(table, leaf_index)?;
        if entry & ENTRY_PRESENT == 0 {
            return Err(FrameError::NotMapped);
        }
        self.access
            .write_entry(table, leaf_index, (entry & ENTRY_ADDRESS_MASK) | flags)
    }

    fn map_inner(
        &mut self,
        virtual_address: u64,
        physical: u64,
        flags: u64,
        user_mapping: bool,
    ) -> Result<(), FrameError> {
        let indices = page_indices(virtual_address)?;
        if !physical.is_multiple_of(PAGE_SIZE) || physical & !ENTRY_ADDRESS_MASK != 0 {
            return Err(FrameError::InvalidPhysicalAddress);
        }
        if flags & !ENTRY_ALLOWED_FLAGS != 0 || flags & ENTRY_PRESENT == 0 {
            return Err(FrameError::InvalidPageFlags);
        }
        let mut table = self.root;
        let mut created: [(u64, usize, u64); 3] = [(0, 0, 0); 3];
        let mut created_len = 0;
        for &index in indices.iter().take(3) {
            let entry = self.access.read_entry(table, index)?;
            if entry & ENTRY_PRESENT != 0 {
                if entry & ENTRY_HUGE != 0 {
                    return Err(FrameError::HugePageConflict);
                }
                let mut effective_entry = entry;
                if user_mapping {
                    effective_entry |= ENTRY_USER;
                    if flags & ENTRY_WRITABLE != 0 {
                        effective_entry |= ENTRY_WRITABLE;
                    }
                }
                if effective_entry != entry {
                    self.access.write_entry(table, index, effective_entry)?;
                }
                table = entry & ENTRY_ADDRESS_MASK;
                continue;
            }
            let child = match self.access.allocate_table() {
                Ok(frame)
                    if frame.is_multiple_of(PAGE_SIZE) && frame & !ENTRY_ADDRESS_MASK == 0 =>
                {
                    frame
                }
                Ok(frame) => {
                    let _ = self.access.release_table(frame);
                    return Err(FrameError::InvalidPhysicalAddress);
                }
                Err(error) => {
                    self.rollback_created(&created[..created_len]);
                    return Err(error);
                }
            };
            if let Err(error) = self.access.write_entry(
                table,
                index,
                child | ENTRY_PRESENT | 2 | if user_mapping { ENTRY_USER } else { 0 },
            ) {
                let _ = self.access.release_table(child);
                self.rollback_created(&created[..created_len]);
                return Err(error);
            }
            created[created_len] = (table, index, child);
            created_len += 1;
            table = child;
        }
        let leaf = indices[3];
        match self.access.read_entry(table, leaf) {
            Ok(entry) if entry & ENTRY_PRESENT != 0 => {
                self.rollback_created(&created[..created_len]);
                Err(FrameError::AlreadyMapped)
            }
            Err(error) => {
                self.rollback_created(&created[..created_len]);
                Err(error)
            }
            _ => match self.access.write_entry(table, leaf, physical | flags) {
                Ok(()) => {
                    self.access.invalidate_page(virtual_address);
                    Ok(())
                }
                Err(error) => {
                    self.rollback_created(&created[..created_len]);
                    Err(error)
                }
            },
        }
    }

    pub fn unmap(&mut self, virtual_address: u64) -> Result<u64, FrameError> {
        let indices = page_indices(virtual_address)?;
        let mut tables = [self.root; 4];
        let mut path_entries = [0; 4];
        for level in 0..3 {
            let entry = self.access.read_entry(tables[level], indices[level])?;
            if entry & ENTRY_PRESENT == 0 {
                return Err(FrameError::NotMapped);
            }
            if entry & ENTRY_HUGE != 0 {
                return Err(FrameError::HugePageConflict);
            }
            path_entries[level] = entry;
            tables[level + 1] = entry & ENTRY_ADDRESS_MASK;
        }
        let leaf = self.access.read_entry(tables[3], indices[3])?;
        if leaf & ENTRY_PRESENT == 0 {
            return Err(FrameError::NotMapped);
        }
        self.access.write_entry(tables[3], indices[3], 0)?;
        self.access.invalidate_page(virtual_address);
        for level in (1..=3).rev() {
            let table = tables[level];
            let mut empty = true;
            for index in 0..PAGE_TABLE_ENTRIES {
                if self.access.read_entry(table, index)? & ENTRY_PRESENT != 0 {
                    empty = false;
                    break;
                }
            }
            if !empty {
                break;
            }
            self.access
                .write_entry(tables[level - 1], indices[level - 1], 0)?;
            if let Err(error) = self.access.release_table(table) {
                // The table may predate this manager. Restore its parent link
                // and leave the now-empty table allocated rather than freeing
                // memory whose ownership is unknown.
                self.access.write_entry(
                    tables[level - 1],
                    indices[level - 1],
                    path_entries[level - 1],
                )?;
                if error == FrameError::NotAllocated {
                    break;
                }
                return Err(error);
            }
        }
        Ok(leaf & ENTRY_ADDRESS_MASK)
    }

    /// Consulta tradução e flags efetivas da folha, útil para validar que uma
    /// página está acessível em user mode ou permanece supervisor.
    pub fn mapping(&self, virtual_address: u64) -> Result<PageMapping, FrameError> {
        let indices = page_indices(virtual_address)?;
        let mut table = self.root;
        let mut effective_flags = ENTRY_PRESENT | ENTRY_USER | ENTRY_WRITABLE;
        for index in indices.iter().take(3) {
            let entry = self.access.read_entry(table, *index)?;
            if entry & ENTRY_PRESENT == 0 {
                return Err(FrameError::NotMapped);
            }
            if entry & ENTRY_HUGE != 0 {
                return Err(FrameError::HugePageConflict);
            }
            effective_flags &= entry;
            effective_flags |= entry & PAGE_FLAG_NO_EXECUTE;
            table = entry & ENTRY_ADDRESS_MASK;
        }
        let entry = self.access.read_entry(table, indices[3])?;
        if entry & ENTRY_PRESENT == 0 {
            return Err(FrameError::NotMapped);
        }
        effective_flags &= entry;
        effective_flags |= entry & PAGE_FLAG_NO_EXECUTE;
        let leaf_flags = entry & !ENTRY_ADDRESS_MASK;
        let effective_permissions = ENTRY_PRESENT
            | (effective_flags & (ENTRY_USER | ENTRY_WRITABLE))
            | (effective_flags & PAGE_FLAG_NO_EXECUTE);
        Ok(PageMapping {
            physical: (entry & ENTRY_ADDRESS_MASK) | (virtual_address & (PAGE_SIZE - 1)),
            flags: (leaf_flags & !(ENTRY_USER | ENTRY_WRITABLE | PAGE_FLAG_NO_EXECUTE))
                | effective_permissions,
        })
    }

    /// Conta a raiz privada e as tabelas inferiores alcançáveis por endereços
    /// user. Entradas de half kernel compartilhado não entram na contagem.
    pub fn user_page_table_pages(&self) -> Result<usize, FrameError> {
        let mut count = 1usize; // raiz PML4 privada
        for index in 0..256 {
            let entry = self.access.read_entry(self.root, index)?;
            if entry & ENTRY_PRESENT == 0 {
                continue;
            }
            if entry & ENTRY_HUGE != 0 {
                return Err(FrameError::HugePageConflict);
            }
            count = count
                .checked_add(self.count_user_table_subtree(entry & ENTRY_ADDRESS_MASK, 1)?)
                .ok_or(FrameError::CapacityExceeded)?;
        }
        Ok(count)
    }

    fn count_user_table_subtree(&self, table: u64, depth: usize) -> Result<usize, FrameError> {
        let mut count = 1usize;
        if depth >= 3 {
            return Ok(count);
        }
        for index in 0..PAGE_TABLE_ENTRIES {
            let entry = self.access.read_entry(table, index)?;
            if entry & ENTRY_PRESENT == 0 {
                continue;
            }
            if entry & ENTRY_HUGE != 0 {
                return Err(FrameError::HugePageConflict);
            }
            count = count
                .checked_add(self.count_user_table_subtree(entry & ENTRY_ADDRESS_MASK, depth + 1)?)
                .ok_or(FrameError::CapacityExceeded)?;
        }
        Ok(count)
    }

    fn protect_user(&mut self, virtual_address: u64, flags: u64) -> Result<(), FrameError> {
        let protection_mask = ENTRY_PRESENT | ENTRY_USER | ENTRY_WRITABLE | PAGE_FLAG_NO_EXECUTE;
        if !virtual_address.is_multiple_of(PAGE_SIZE)
            || flags & !protection_mask != 0
            || flags & (ENTRY_PRESENT | ENTRY_USER) != (ENTRY_PRESENT | ENTRY_USER)
        {
            return Err(FrameError::InvalidPageFlags);
        }
        let indices = page_indices(virtual_address)?;
        let mut table = self.root;
        for index in indices.iter().take(3) {
            let entry = self.access.read_entry(table, *index)?;
            if entry & ENTRY_PRESENT == 0 {
                return Err(FrameError::NotMapped);
            }
            if entry & ENTRY_HUGE != 0 {
                return Err(FrameError::HugePageConflict);
            }
            table = entry & ENTRY_ADDRESS_MASK;
        }
        let entry = self.access.read_entry(table, indices[3])?;
        if entry & ENTRY_PRESENT == 0 {
            return Err(FrameError::NotMapped);
        }
        self.access.write_entry(
            table,
            indices[3],
            (entry & ENTRY_ADDRESS_MASK) | flags | ENTRY_USER,
        )?;
        self.access.invalidate_page(virtual_address);
        Ok(())
    }

    fn rollback_created(&mut self, created: &[(u64, usize, u64)]) {
        for &(parent, index, child) in created.iter().rev() {
            let _ = self.access.write_entry(parent, index, 0);
            let _ = self.access.release_table(child);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageMapping {
    pub physical: u64,
    pub flags: u64,
}

fn page_indices(address: u64) -> Result<[usize; 4], FrameError> {
    let upper = address >> 48;
    let sign = (address >> 47) & 1;
    if (sign == 0 && upper != 0) || (sign == 1 && upper != 0xffff) {
        return Err(FrameError::InvalidVirtualAddress);
    }
    Ok([
        ((address >> 39) & 0x1ff) as usize,
        ((address >> 30) & 0x1ff) as usize,
        ((address >> 21) & 0x1ff) as usize,
        ((address >> 12) & 0x1ff) as usize,
    ])
}

const PAGE_SIZE: u64 = 4096;
const LOW_MEMORY_LIMIT: u64 = 1024 * 1024;
const LOW_MEMORY_FRAME: u64 = LOW_MEMORY_LIMIT / PAGE_SIZE;
/// Limite superior exclusivo do espaço físico que o kernel se propõe a gerir.
/// 32 GiB cobrem a capacidade alvo sem reservar bitmap para todo o espaço de
/// endereços físico de 52 bits.
pub const MAX_PHYSICAL_ADDRESS: u64 = 32 * 1024 * 1024 * 1024;
pub const PHYSICAL_FRAME_BITMAP_WORDS: usize =
    ((MAX_PHYSICAL_ADDRESS / PAGE_SIZE - LOW_MEMORY_FRAME) as usize) / 64;

/// Physical page allocator bootstrapped from the usable Limine regions.
/// Availability and ownership have separate bitmaps so reserved pages cannot
/// be freed accidentally. Its mutation requires external serialization; the
/// kernel-wide instance is protected by `IrqSpinLock`.
pub struct PhysicalFrameAllocator<const WORDS: usize> {
    available: [u64; WORDS],
    allocated: [u64; WORDS],
    free_frames: usize,
    allocated_frames: usize,
}

impl<const WORDS: usize> PhysicalFrameAllocator<WORDS> {
    pub const fn new() -> Self {
        Self {
            available: [0; WORDS],
            allocated: [0; WORDS],
            free_frames: 0,
            allocated_frames: 0,
        }
    }

    /// Makes only complete pages from a usable firmware/bootloader region
    /// available. Memory below 1 MiB and addresses at/above 32 GiB stay reserved.
    /// Returns the number of newly available pages.
    pub fn add_usable_region(&mut self, base: u64, length: u64) -> Result<usize, FrameError> {
        let end = base.checked_add(length).ok_or(FrameError::InvalidRegion)?;
        if length == 0 || end <= LOW_MEMORY_LIMIT {
            return Ok(0);
        }
        let first = base.max(LOW_MEMORY_LIMIT).div_ceil(PAGE_SIZE);
        let limit = LOW_MEMORY_FRAME
            .checked_add(
                (WORDS as u64)
                    .checked_mul(64)
                    .ok_or(FrameError::CapacityExceeded)?,
            )
            .ok_or(FrameError::CapacityExceeded)?;
        let last = (end / PAGE_SIZE).min(limit);
        let mut added = 0;
        for frame in first..last {
            let index = (frame - LOW_MEMORY_FRAME) as usize;
            let mask = 1u64 << (index % 64);
            let word = &mut self.available[index / 64];
            if *word & mask == 0 {
                *word |= mask;
                added += 1;
            }
        }
        self.free_frames += added;
        Ok(added)
    }

    pub fn allocate(&mut self) -> Option<u64> {
        self.allocate_at_or_above(LOW_MEMORY_LIMIT)
    }

    /// Aloca o primeiro frame livre em ou acima do endereço solicitado. Usado
    /// para provar e atender memória alta sem percorrer/consumir frames baixos.
    pub fn allocate_at_or_above(&mut self, minimum_address: u64) -> Option<u64> {
        let first_frame = minimum_address
            .div_ceil(PAGE_SIZE)
            .saturating_sub(LOW_MEMORY_FRAME);
        let first_index = usize::try_from(first_frame).ok()?;
        if first_index >= WORDS.checked_mul(64)? {
            return None;
        }
        for word_index in first_index / 64..WORDS {
            let mut candidates = self.available[word_index] & !self.allocated[word_index];
            if word_index == first_index / 64 {
                candidates &= u64::MAX << (first_index % 64);
            }
            if candidates == 0 {
                continue;
            }
            let bit = candidates.trailing_zeros() as usize;
            let frame = word_index.checked_mul(64)?.checked_add(bit)?;
            let address = (frame as u64 + LOW_MEMORY_FRAME).checked_mul(PAGE_SIZE)?;
            self.allocated[word_index] |= 1u64 << bit;
            self.free_frames -= 1;
            self.allocated_frames += 1;
            return Some(address);
        }
        None
    }

    pub fn free(&mut self, address: u64) -> Result<(), FrameError> {
        if !address.is_multiple_of(PAGE_SIZE) {
            return Err(FrameError::InvalidFrame);
        }
        let physical_frame = address / PAGE_SIZE;
        if physical_frame < LOW_MEMORY_FRAME {
            return Err(FrameError::InvalidFrame);
        }
        let frame = usize::try_from(physical_frame - LOW_MEMORY_FRAME)
            .map_err(|_| FrameError::InvalidFrame)?;
        if frame >= WORDS.saturating_mul(64) {
            return Err(FrameError::InvalidFrame);
        }
        let mask = 1u64 << (frame % 64);
        if self.available[frame / 64] & mask == 0 {
            return Err(FrameError::InvalidFrame);
        }
        let owned = &mut self.allocated[frame / 64];
        if *owned & mask == 0 {
            return Err(FrameError::NotAllocated);
        }
        *owned &= !mask;
        self.free_frames += 1;
        self.allocated_frames -= 1;
        Ok(())
    }

    pub const fn free_frames(&self) -> usize {
        self.free_frames
    }

    pub const fn allocated_frames(&self) -> usize {
        self.allocated_frames
    }
}

impl<const WORDS: usize> Default for PhysicalFrameAllocator<WORDS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const WORDS: usize> FrameAllocator<WORDS> {
    pub const fn new(base: u64, frame_count: usize) -> Result<Self, FrameError> {
        if base % 4096 != 0 {
            return Err(FrameError::UnalignedBase);
        }
        if frame_count > WORDS * 64 {
            return Err(FrameError::CapacityExceeded);
        }
        Ok(Self {
            base,
            frame_count,
            bitmap: [0; WORDS],
            allocated: 0,
        })
    }

    pub fn allocate(&mut self) -> Option<u64> {
        for index in 0..self.frame_count {
            let mask = 1u64 << (index % 64);
            let word = &mut self.bitmap[index / 64];
            if *word & mask == 0 {
                let address = self.base.checked_add((index as u64).checked_mul(4096)?)?;
                *word |= mask;
                self.allocated += 1;
                return Some(address);
            }
        }
        None
    }

    pub fn free(&mut self, address: u64) -> Result<(), FrameError> {
        let offset = address
            .checked_sub(self.base)
            .ok_or(FrameError::InvalidFrame)?;
        if offset % 4096 != 0 {
            return Err(FrameError::InvalidFrame);
        }
        let index = usize::try_from(offset / 4096).map_err(|_| FrameError::InvalidFrame)?;
        if index >= self.frame_count {
            return Err(FrameError::InvalidFrame);
        }
        let mask = 1u64 << (index % 64);
        let word = &mut self.bitmap[index / 64];
        if *word & mask == 0 {
            return Err(FrameError::AlreadyFree);
        }
        *word &= !mask;
        self.allocated -= 1;
        Ok(())
    }

    pub const fn allocated(&self) -> usize {
        self.allocated
    }

    pub const fn capacity(&self) -> usize {
        self.frame_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static HIGH_MEMORY_ALLOCATOR: std::sync::Mutex<
        PhysicalFrameAllocator<PHYSICAL_FRAME_BITMAP_WORDS>,
    > = std::sync::Mutex::new(PhysicalFrameAllocator::new());
    static UPPER_MEMORY_ALLOCATOR: std::sync::Mutex<
        PhysicalFrameAllocator<PHYSICAL_FRAME_BITMAP_WORDS>,
    > = std::sync::Mutex::new(PhysicalFrameAllocator::new());
    static HIGH_FRAME_SELECTION_ALLOCATOR: std::sync::Mutex<
        PhysicalFrameAllocator<PHYSICAL_FRAME_BITMAP_WORDS>,
    > = std::sync::Mutex::new(PhysicalFrameAllocator::new());

    #[derive(Default)]
    struct FakePageTables {
        entries: std::collections::BTreeMap<(u64, usize), u64>,
        next: u64,
        allocated: std::collections::BTreeSet<u64>,
        invalidated: Vec<u64>,
    }

    impl FakePageTables {
        fn new() -> Self {
            Self::with_next(0x20_0000)
        }

        fn with_next(next: u64) -> Self {
            Self {
                next,
                ..Self::default()
            }
        }
    }

    impl PageTableAccess for FakePageTables {
        fn read_entry(&self, table: u64, index: usize) -> Result<u64, FrameError> {
            Ok(self.entries.get(&(table, index)).copied().unwrap_or(0))
        }
        fn write_entry(&mut self, table: u64, index: usize, value: u64) -> Result<(), FrameError> {
            self.entries.insert((table, index), value);
            Ok(())
        }
        fn allocate_table(&mut self) -> Result<u64, FrameError> {
            let frame = self.next;
            self.next += PAGE_SIZE;
            self.allocated.insert(frame);
            Ok(frame)
        }
        fn release_table(&mut self, physical: u64) -> Result<(), FrameError> {
            self.allocated
                .remove(&physical)
                .then_some(())
                .ok_or(FrameError::NotAllocated)
        }
        fn invalidate_page(&mut self, virtual_address: u64) {
            self.invalidated.push(virtual_address);
        }
    }

    struct CountingShootdown(std::sync::atomic::AtomicUsize);

    impl TlbShootdown for CountingShootdown {
        fn invalidate_all(&self, _root: u64, _virtual_address: u64) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    #[test]
    fn physical_allocator_lock_prevents_duplicate_frames_under_contention() {
        use std::sync::Arc;

        let allocator = Arc::new(crate::sync::IrqSpinLock::new(
            PhysicalFrameAllocator::<16>::new(),
        ));
        allocator
            .lock()
            .add_usable_region(LOW_MEMORY_LIMIT, PAGE_SIZE * 1024)
            .unwrap();
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let allocator = Arc::clone(&allocator);
                std::thread::spawn(move || {
                    (0..128)
                        .map(|_| allocator.lock().allocate().unwrap())
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        let allocated: Vec<u64> = workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect();
        let unique: std::collections::BTreeSet<_> = allocated.iter().copied().collect();
        assert_eq!(unique.len(), allocated.len());
        assert_eq!(allocated.len(), 1024);
        let mut allocator = allocator.lock();
        for frame in allocated {
            allocator.free(frame).unwrap();
        }
        assert_eq!(allocator.free_frames(), 1024);
    }

    #[test]
    fn concurrent_vm_edits_are_serialized_and_request_remote_invalidations() {
        use std::sync::atomic::Ordering;
        use std::sync::Arc;

        let vm = Arc::new(
            SmpPageTableManager::new(
                FakePageTables::new(),
                0x10_0000,
                CountingShootdown(std::sync::atomic::AtomicUsize::new(0)),
            )
            .unwrap(),
        );
        let workers: Vec<_> = (0..4)
            .map(|worker| {
                let vm = Arc::clone(&vm);
                std::thread::spawn(move || {
                    for index in 0..128u64 {
                        let virtual_address = 0x4000_0000 + (worker * 128 + index) * PAGE_SIZE;
                        let physical = 0x8000_0000 + (worker * 128 + index) * PAGE_SIZE;
                        vm.map(virtual_address, physical, 3).unwrap();
                        assert_eq!(vm.mapping(virtual_address).unwrap().physical, physical);
                        assert_eq!(vm.unmap(virtual_address).unwrap(), physical);
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(vm.shootdown.0.load(Ordering::Relaxed), 4 * 128 * 2);
    }

    #[test]
    fn smp_protection_waits_for_shootdown_and_preserves_frame() {
        use std::sync::atomic::Ordering;

        let vm = SmpPageTableManager::new(
            FakePageTables::new(),
            0x10_0000,
            CountingShootdown(std::sync::atomic::AtomicUsize::new(0)),
        )
        .unwrap();
        let va = 0xffff_c100_0000_0000;
        let pa = 0x9000_0000;
        vm.map(va, pa, PAGE_FLAG_WRITABLE | 1).unwrap();
        vm.protect(va, PAGE_FLAG_NO_EXECUTE | 1).unwrap();
        let mapping = vm.mapping(va).unwrap();
        assert_eq!(mapping.physical, pa);
        assert_eq!(mapping.flags & PAGE_FLAG_WRITABLE, 0);
        assert_ne!(mapping.flags & PAGE_FLAG_NO_EXECUTE, 0);
        assert_eq!(vm.shootdown.0.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn translates_kernel_addresses_with_checked_arithmetic() {
        let layout = KernelAddress {
            physical_base: 0x20_0000,
            virtual_base: 0xffff_ffff_8000_0000,
        };
        assert_eq!(
            layout.virtual_to_physical(0xffff_ffff_8000_1234),
            Some(0x20_1234)
        );
        assert_eq!(layout.virtual_to_physical(0xffff_ffff_7fff_ffff), None);
        assert_eq!(
            KernelAddress {
                physical_base: u64::MAX,
                ..layout
            }
            .virtual_to_physical(layout.virtual_base + 1),
            None
        );
    }

    #[test]
    fn hhdm_translation_rejects_overflow() {
        assert_eq!(
            hhdm_address(0xffff_8000_0000_0000, 0x1234),
            Some(0xffff_8000_0000_1234)
        );
        assert_eq!(hhdm_address(u64::MAX, 1), None);
    }

    #[test]
    fn frame_allocator_allocates_and_reuses_aligned_frames() {
        let mut allocator = FrameAllocator::<1>::new(0x20_0000, 3).unwrap();
        assert_eq!(allocator.allocate(), Some(0x20_0000));
        assert_eq!(allocator.allocate(), Some(0x20_1000));
        assert_eq!(allocator.allocate(), Some(0x20_2000));
        assert_eq!(allocator.allocate(), None);
        allocator.free(0x20_1000).unwrap();
        assert_eq!(allocator.allocate(), Some(0x20_1000));
        assert_eq!(allocator.allocated(), 3);
    }

    #[test]
    fn frame_allocator_rejects_invalid_and_double_frees() {
        let mut allocator = FrameAllocator::<1>::new(0x20_0000, 2).unwrap();
        assert!(matches!(
            FrameAllocator::<1>::new(1, 1),
            Err(FrameError::UnalignedBase)
        ));
        assert!(matches!(
            FrameAllocator::<1>::new(0, 65),
            Err(FrameError::CapacityExceeded)
        ));
        assert_eq!(allocator.free(0x20_1001), Err(FrameError::InvalidFrame));
        assert_eq!(allocator.free(0x20_2000), Err(FrameError::InvalidFrame));
        assert_eq!(allocator.free(0x20_0000), Err(FrameError::AlreadyFree));
        allocator.allocate().unwrap();
        allocator.free(0x20_0000).unwrap();
        assert_eq!(allocator.free(0x20_0000), Err(FrameError::AlreadyFree));
    }

    #[test]
    fn physical_allocator_uses_only_usable_aligned_frames() {
        let mut allocator = PhysicalFrameAllocator::<2>::new();
        assert_eq!(allocator.add_usable_region(0, 1024 * 1024), Ok(0));
        assert_eq!(allocator.add_usable_region(0x10_0001, 0x5000), Ok(4));
        assert_eq!(allocator.free_frames(), 4);
        assert_eq!(allocator.allocate(), Some(0x10_1000));
        assert_eq!(allocator.allocate(), Some(0x10_2000));
        assert_eq!(allocator.allocated_frames(), 2);
    }

    #[test]
    fn physical_allocator_rejects_reserved_invalid_and_double_frees() {
        let mut allocator = PhysicalFrameAllocator::<1024>::new();
        allocator.add_usable_region(0x20_0000, 0x2000).unwrap();
        assert_eq!(allocator.free(0x20_0000), Err(FrameError::NotAllocated));
        assert_eq!(allocator.allocate(), Some(0x20_0000));
        assert_eq!(allocator.free(0x20_0001), Err(FrameError::InvalidFrame));
        assert_eq!(allocator.free(0x21_0000), Err(FrameError::InvalidFrame));
        allocator.free(0x20_0000).unwrap();
        assert_eq!(allocator.free(0x20_0000), Err(FrameError::NotAllocated));
        assert_eq!(allocator.free_frames(), 2);
    }

    #[test]
    fn physical_allocator_deduplicates_regions_and_clips_capacity() {
        let mut allocator = PhysicalFrameAllocator::<1>::new();
        assert_eq!(allocator.add_usable_region(0x10_0000, 0x3000), Ok(3));
        assert_eq!(allocator.add_usable_region(0x10_1000, 0x1000), Ok(0));
        assert_eq!(allocator.add_usable_region(0xffff_f000, 0x4000), Ok(0));
        assert_eq!(
            allocator.add_usable_region(u64::MAX - 1, 4),
            Err(FrameError::InvalidRegion)
        );
        assert_eq!(allocator.free_frames(), 3);
    }

    #[test]
    fn physical_allocator_reaches_above_four_gib_and_clips_at_32_gib() {
        let mut allocator = HIGH_MEMORY_ALLOCATOR.lock().unwrap();
        let four_gib = 4 * 1024 * 1024 * 1024;
        assert_eq!(allocator.add_usable_region(four_gib, 2 * PAGE_SIZE), Ok(2));
        assert_eq!(allocator.allocate(), Some(four_gib));
        assert_eq!(allocator.allocate(), Some(four_gib + PAGE_SIZE));
        allocator.free(four_gib).unwrap();

        let mut allocator = UPPER_MEMORY_ALLOCATOR.lock().unwrap();
        let last_two_pages = MAX_PHYSICAL_ADDRESS - 2 * PAGE_SIZE;
        assert_eq!(
            allocator.add_usable_region(last_two_pages, 3 * PAGE_SIZE),
            Ok(2)
        );
        assert_eq!(allocator.allocate(), Some(last_two_pages));
        assert_eq!(allocator.allocate(), Some(last_two_pages + PAGE_SIZE));
        assert_eq!(allocator.allocate(), None);
    }

    #[test]
    fn physical_allocator_can_request_a_high_frame_without_consuming_low_memory() {
        let mut allocator = HIGH_FRAME_SELECTION_ALLOCATOR.lock().unwrap();
        let high = 4 * 1024 * 1024 * 1024;
        allocator.add_usable_region(high, PAGE_SIZE).unwrap();
        assert_eq!(allocator.allocate_at_or_above(high), Some(high));
        assert_eq!(allocator.free_frames(), 0);
    }

    #[test]
    fn page_table_map_unmap_validates_and_reclaims_empty_tables() {
        let root = 0x1000;
        let mut manager = PageTableManager::new(FakePageTables::new(), root).unwrap();
        let va = 0xffff_a000_0000_0000;
        let pa = 0x4000_0000;
        assert_eq!(manager.map(va, pa, ENTRY_PRESENT | 2), Ok(()));
        assert_eq!(
            manager.map(va, pa + PAGE_SIZE, ENTRY_PRESENT | 2),
            Err(FrameError::AlreadyMapped)
        );
        assert_eq!(manager.unmap(va), Ok(pa));
        assert_eq!(manager.unmap(va), Err(FrameError::NotMapped));
        let access = manager.into_access();
        assert!(access.allocated.is_empty());
        assert_eq!(access.invalidated, [va, va]);
    }

    #[test]
    fn owned_address_space_reclaims_user_frames_and_page_tables() {
        let kernel_root = 0x1000;
        let mut space = OwnedAddressSpace::<_, 2>::new(FakePageTables::new(), kernel_root).unwrap();
        let root = space.root();
        let code = space.map_zeroed(0x400000, 0x1).unwrap();
        let stack = space.map_zeroed(0x800000, 0x3).unwrap();
        assert_ne!(code, stack);
        assert_eq!(space.mapping(0x400000).unwrap().physical, code);
        assert_eq!(space.mapping(0x800000).unwrap().physical, stack);
        assert_eq!(
            space.map_zeroed(0x800000, 0x3),
            Err(FrameError::AlreadyMapped)
        );
        assert_eq!(space.unmap(0x800000), Ok(()));
        assert_eq!(space.mapping(0x800000), Err(FrameError::NotMapped));
        let access = match space.try_destroy() {
            Ok(access) => access,
            Err((_, error)) => panic!("destruição do espaço falhou: {error:?}"),
        };
        assert!(!access.allocated.contains(&root));
        assert!(!access.allocated.contains(&code));
        assert!(!access.allocated.contains(&stack));
        assert!(access.allocated.is_empty());
    }

    #[test]
    fn host_model_isolates_processes_mapping_the_same_user_address() {
        let kernel_root = 0x1000;
        let virtual_page = 0x400000;
        let mut first =
            AddressSpace::<_, 2>::new(FakePageTables::with_next(0x20_0000), kernel_root).unwrap();
        let mut second =
            AddressSpace::<_, 2>::new(FakePageTables::with_next(0x40_0000), kernel_root).unwrap();

        first
            .map_user(
                virtual_page,
                0x80_0000,
                ENTRY_PRESENT | ENTRY_USER | ENTRY_WRITABLE,
            )
            .unwrap();
        second
            .map_user(
                virtual_page,
                0x90_0000,
                ENTRY_PRESENT | ENTRY_USER | ENTRY_WRITABLE,
            )
            .unwrap();

        assert_eq!(first.mapping(virtual_page).unwrap().physical, 0x80_0000);
        assert_eq!(second.mapping(virtual_page).unwrap().physical, 0x90_0000);
        assert_ne!(first.root(), second.root());

        let first_access = match first.try_destroy() {
            Ok(access) => access,
            Err((_, error)) => panic!("destruição do primeiro espaço falhou: {error:?}"),
        };
        let second_access = match second.try_destroy() {
            Ok(access) => access,
            Err((_, error)) => panic!("destruição do segundo espaço falhou: {error:?}"),
        };
        assert!(first_access.allocated.is_empty());
        assert!(second_access.allocated.is_empty());
    }

    #[test]
    fn page_table_rejects_bad_addresses_flags_and_huge_conflicts() {
        let root = 0x1000;
        let mut access = FakePageTables::new();
        access
            .entries
            .insert((root, 1), 0x2000 | ENTRY_PRESENT | ENTRY_HUGE);
        let mut manager = PageTableManager::new(access, root).unwrap();
        assert_eq!(
            manager.map(0x0001_0000_0000_0000, 0x4000, 3),
            Err(FrameError::InvalidVirtualAddress)
        );
        assert_eq!(
            manager.map(0xffff_a000_0000_0000, 0x4001, 3),
            Err(FrameError::InvalidPhysicalAddress)
        );
        assert_eq!(
            manager.map(0xffff_a000_0000_0000, 0x4000, 7),
            Err(FrameError::InvalidPageFlags)
        );
        assert_eq!(
            manager.map(0x0000_8000_0000_0000, 0x4000, 3),
            Err(FrameError::InvalidVirtualAddress)
        );
        assert_eq!(manager.map(0x0000_0000_8000_0000, 0x4000, 3), Ok(()));
        assert_eq!(
            manager.map(0x0000_0000_8000_0000 | (1 << 39), 0x4000, 3),
            Err(FrameError::HugePageConflict)
        );
    }

    #[test]
    fn user_mapping_sets_user_access_on_path_and_keeps_kernel_leaf_supervisor() {
        let root = 0x1000;
        let mut manager = PageTableManager::new(FakePageTables::new(), root).unwrap();
        let user_va = 0x0040_0000;
        manager
            .map_user(user_va, 0x4000, ENTRY_PRESENT | 2)
            .unwrap();
        let user_map = manager.mapping(user_va).unwrap();
        assert_eq!(user_map.physical, 0x4000);
        assert_ne!(user_map.flags & ENTRY_USER, 0);
        assert_ne!(user_map.flags & 2, 0);
        assert_eq!(
            manager.mapping(user_va + 1),
            Ok(PageMapping {
                physical: 0x4001,
                flags: user_map.flags,
            })
        );

        let kernel_va = 0xffff_a000_0000_0000;
        manager.map(kernel_va, 0x8000, ENTRY_PRESENT | 2).unwrap();
        assert_eq!(manager.mapping(kernel_va).unwrap().flags & ENTRY_USER, 0);
    }

    #[test]
    fn mapping_reports_effective_user_permissions_from_every_level() {
        let root = 0x1000;
        let mut access = FakePageTables::new();
        // A user-marked leaf is still supervisor-only when an ancestor lacks U/S.
        access
            .entries
            .insert((root, 0), 0x2000 | ENTRY_PRESENT | ENTRY_WRITABLE);
        access.entries.insert(
            (0x2000, 1),
            0x3000 | ENTRY_PRESENT | ENTRY_USER | ENTRY_WRITABLE,
        );
        access
            .entries
            .insert((0x3000, 0), 0x4000 | ENTRY_PRESENT | ENTRY_USER);
        access.entries.insert(
            (0x4000, 0),
            0x8000 | ENTRY_PRESENT | ENTRY_USER | ENTRY_WRITABLE,
        );
        let manager = PageTableManager::new(access, root).unwrap();
        let mapping = manager.mapping(0x4000_0000).unwrap();
        assert_eq!(mapping.flags & ENTRY_USER, 0);
        assert_eq!(mapping.flags & ENTRY_WRITABLE, 0);
    }

    #[test]
    fn user_mapping_rejects_kernel_half_and_legacy_map_rejects_user_flag() {
        let mut manager = PageTableManager::new(FakePageTables::new(), 0x1000).unwrap();
        assert_eq!(
            manager.map_user(0xffff_8000_0000_0000, 0x4000, ENTRY_PRESENT | 2),
            Err(FrameError::InvalidVirtualAddress)
        );
        assert_eq!(
            manager.map(0x4000, 0x4000, ENTRY_PRESENT | 2 | ENTRY_USER),
            Err(FrameError::InvalidPageFlags)
        );
    }

    #[test]
    fn address_space_copies_kernel_half_and_owns_only_user_page_tables() {
        let kernel_root = 0x1000;
        let kernel_entry = 0x3000 | ENTRY_PRESENT | 2;
        let mut access = FakePageTables::new();
        access.entries.insert((kernel_root, 511), kernel_entry);
        let mut space = AddressSpace::<_, 1>::new(access, kernel_root).unwrap();
        let process_root = space.root();
        assert_ne!(process_root, kernel_root);
        space.map_user(0x4000, 0x8000, ENTRY_PRESENT | 2).unwrap();
        assert_eq!(space.mapping(0x4000).unwrap().physical, 0x8000);
        assert_eq!(
            space.map_user(0x5000, 0x9000, ENTRY_PRESENT | 2),
            Err(FrameError::CapacityExceeded)
        );
        assert_eq!(space.unmap_user(0x5000), Err(FrameError::NotMapped));
        assert_eq!(space.unmap_user(0x4000), Ok(0x8000));
        let access = match space.try_destroy() {
            Ok(access) => access,
            Err((_, error)) => panic!("falha ao destruir espaço de teste: {error:?}"),
        };
        assert_eq!(
            access.entries.get(&(process_root, 511)),
            Some(&kernel_entry)
        );
        assert!(!access.allocated.contains(&process_root));
        assert!(access.allocated.is_empty());
        assert_eq!(access.entries.get(&(kernel_root, 511)), Some(&kernel_entry));
    }

    #[test]
    fn address_space_forces_inherited_kernel_half_to_supervisor() {
        let kernel_root = 0x1000;
        let kernel_entry = 0x3000 | ENTRY_PRESENT | ENTRY_USER | ENTRY_WRITABLE;
        let mut access = FakePageTables::new();
        access.entries.insert((kernel_root, 511), kernel_entry);
        let space = AddressSpace::<_, 1>::new(access, kernel_root).unwrap();
        let process_root_entry = space.tables.access.read_entry(space.root(), 511).unwrap();
        assert_eq!(process_root_entry & ENTRY_USER, 0);
        assert_eq!(process_root_entry & !ENTRY_USER, kernel_entry & !ENTRY_USER);
        let access = match space.try_destroy() {
            Ok(access) => access,
            Err((_, error)) => panic!("falha ao destruir espaço de teste: {error:?}"),
        };
        assert_eq!(access.entries.get(&(kernel_root, 511)), Some(&kernel_entry));
    }

    #[test]
    fn user_mapping_promotes_existing_path_and_unmap_preserves_unowned_tables() {
        let root = 0x1000;
        let mut access = FakePageTables::new();
        access.entries.insert((root, 0), 0x2000 | ENTRY_PRESENT);
        access.entries.insert((0x2000, 0), 0x3000 | ENTRY_PRESENT);
        access.entries.insert((0x3000, 0), 0x4000 | ENTRY_PRESENT);
        let mut manager = PageTableManager::new(access, root).unwrap();
        manager
            .map_user(0x4000, 0x8000, ENTRY_PRESENT | ENTRY_WRITABLE)
            .unwrap();
        for (table, index) in [(root, 0), (0x2000, 0), (0x3000, 0)] {
            let entry = manager.access.read_entry(table, index).unwrap();
            assert_ne!(entry & ENTRY_USER, 0);
            assert_ne!(entry & ENTRY_WRITABLE, 0);
        }
        assert_eq!(manager.unmap(0x4000), Ok(0x8000));
        let access = manager.into_access();
        assert!(access.allocated.is_empty());
        assert_ne!(access.entries[&(root, 0)] & ENTRY_PRESENT, 0);
        assert_ne!(access.entries[&(0x2000, 0)] & ENTRY_PRESENT, 0);
        assert_ne!(access.entries[&(0x3000, 0)] & ENTRY_PRESENT, 0);
    }
}

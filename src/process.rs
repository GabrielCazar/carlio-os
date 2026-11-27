//! Tabela fixa de processos e fila de execução compartilhada entre CPUs.
//! Escalonador e contrato de recursos por processo. A camada x86_64 aplica o
//! frame de interrupção, CR3 e RSP0 escolhidos pelo escalonador.
use crate::sync::IrqSpinLock;
use crate::vfs::Credentials;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU64, Ordering};

/// Bytes occupied by the x86_64 return frame plus all saved general registers.
pub const USER_INTERRUPT_FRAME_BYTES: u64 = 22 * 8;

/// Pilhas kernel fixas, alinhadas e exclusivas. O token retornado por `allocate`
/// é transferido ao scheduler junto com o processo; nenhum slot pode ser
/// alocado duas vezes enquanto estiver em uso.
pub struct KernelStackPool<const N: usize, const BYTES: usize> {
    stacks: [KernelStackStorage<BYTES>; N],
    used: IrqSpinLock<[bool; N]>,
}

/// Pilha descendente com uma página exclusiva imediatamente abaixo dos bytes úteis.
#[repr(C, align(4096))]
pub struct GuardedKernelStack<const BYTES: usize>([u8; 4096], pub [u8; BYTES]);

impl<const BYTES: usize> GuardedKernelStack<BYTES> {
    pub const ZERO: Self = Self([0; 4096], [0; BYTES]);
}

struct KernelStackStorage<const BYTES: usize>(UnsafeCell<GuardedKernelStack<BYTES>>);

impl<const BYTES: usize> KernelStackStorage<BYTES> {
    fn bytes(&self) -> *mut u8 {
        // Compute the field address without accessing the unmapped guard.
        unsafe { core::ptr::addr_of_mut!((*self.0.get()).1).cast() }
    }
}

// Allocation and release serialize ownership. Only the CPU that owns a
// live stack may touch its bytes until it has switched to another stack.
unsafe impl<const BYTES: usize> Sync for KernelStackStorage<BYTES> {}

pub struct KernelStack {
    slot: usize,
    top: u64,
}

/// Barreiria a liberação de recursos até cada CPU requerida confirmar que
/// deixou a stack e o CR3 do processo. O backend só deve confirmar depois de
/// validar esses dois estados arquiteturais.
pub struct ReapBarrier<const C: usize> {
    next_epoch: AtomicU64,
    requested_epoch: AtomicU64,
    acknowledgements: [AtomicU64; C],
}

impl<const C: usize> ReapBarrier<C> {
    pub const fn new() -> Self {
        Self {
            next_epoch: AtomicU64::new(0),
            requested_epoch: AtomicU64::new(0),
            acknowledgements: [const { AtomicU64::new(0) }; C],
        }
    }

    pub fn begin(&self) -> Option<u64> {
        if self.requested_epoch.load(Ordering::Acquire) != 0 {
            return None;
        }
        let mut current = self.next_epoch.load(Ordering::Acquire);
        let epoch = loop {
            let next = current.checked_add(1)?;
            match self.next_epoch.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break next,
                Err(observed) => current = observed,
            }
        };
        self.requested_epoch
            .compare_exchange(0, epoch, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| epoch)
    }

    pub fn requested(&self) -> Option<u64> {
        match self.requested_epoch.load(Ordering::Acquire) {
            0 => None,
            epoch => Some(epoch),
        }
    }

    pub fn acknowledge(&self, cpu: usize, epoch: u64) -> bool {
        let Some(slot) = self.acknowledgements.get(cpu) else {
            return false;
        };
        if epoch == 0 || self.requested_epoch.load(Ordering::Acquire) != epoch {
            return false;
        }
        slot.store(epoch, Ordering::Release);
        true
    }

    pub fn complete(&self, epoch: u64, required_cpus: u64) -> bool {
        if epoch == 0 || self.requested_epoch.load(Ordering::Acquire) != epoch {
            return false;
        }
        for (cpu, acknowledgement) in self.acknowledgements.iter().enumerate() {
            let Ok(bit) = u32::try_from(cpu) else {
                return false;
            };
            let Some(cpu_bit) = 1u64.checked_shl(bit) else {
                return false;
            };
            if required_cpus & cpu_bit != 0 && acknowledgement.load(Ordering::Acquire) != epoch {
                return false;
            }
        }
        true
    }

    pub fn release(&self, epoch: u64) -> bool {
        epoch != 0
            && self
                .requested_epoch
                .compare_exchange(epoch, 0, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }
}

impl<const C: usize> Default for ReapBarrier<C> {
    fn default() -> Self {
        Self::new()
    }
}

impl KernelStack {
    pub const fn top(&self) -> u64 {
        self.top
    }
}

impl<const N: usize, const BYTES: usize> KernelStackPool<N, BYTES> {
    pub const fn new() -> Self {
        Self {
            stacks: [const { KernelStackStorage(UnsafeCell::new(GuardedKernelStack::ZERO)) }; N],
            used: IrqSpinLock::new([false; N]),
        }
    }

    pub fn allocate(&self) -> Option<KernelStack> {
        if BYTES < 4096 || BYTES % 4096 != 0 {
            return None;
        }
        let mut used = self.used.lock();
        let slot = used.iter().position(|used| !used)?;
        let top = self.stack_base(slot).checked_add(BYTES as u64)?;
        // The metadata lock still owns this unpublished slot.
        unsafe { self.stacks[slot].bytes().write_bytes(0xa5, BYTES) };
        used[slot] = true;
        Some(KernelStack { slot, top })
    }

    /// Returns the observed stack high-water mark since this slot was allocated.
    ///
    /// # Safety
    /// The stack must be the current CPU's stack or quiescent. The caller must
    /// prevent any other CPU from running on it while the bytes are read.
    pub unsafe fn high_water_bytes_for_top(&self, top: u64) -> Option<usize> {
        let used = self.used.lock();
        let slot = used.iter().enumerate().find_map(|(slot, used)| {
            (*used && self.stack_base(slot) + BYTES as u64 == top).then_some(slot)
        })?;
        let stack = self.stacks[slot].bytes();
        let first_modified = (0..BYTES)
            .position(|offset| unsafe { stack.add(offset).read_volatile() } != 0xa5)
            .unwrap_or(BYTES);
        Some(BYTES - first_modified)
    }

    /// Liberação apenas após o processo deixar de executar nesta pilha.
    pub fn release(&self, stack: KernelStack) -> Result<(), ProcessError> {
        self.release_slot(stack.slot, stack.top)
    }

    fn release_slot(&self, slot: usize, top: u64) -> Result<(), ProcessError> {
        let mut used = self.used.lock();
        if slot >= N || !used[slot] {
            return Err(ProcessError::UnknownProcess);
        }
        if top != self.stack_base(slot) + BYTES as u64 {
            return Err(ProcessError::NotRunnable);
        }
        used[slot] = false;
        Ok(())
    }

    #[cfg(target_os = "none")]
    pub fn guard_pages(&self) -> impl Iterator<Item = u64> + '_ {
        self.stacks.iter().map(|stack| stack.0.get() as u64)
    }

    fn stack_base(&self, slot: usize) -> u64 {
        #[cfg(target_os = "none")]
        {
            self.stacks[slot].bytes() as u64
        }
        #[cfg(not(target_os = "none"))]
        {
            0xffff_8000_0000_0000u64 + (slot as u64) * BYTES as u64
        }
    }
}

impl<const N: usize, const BYTES: usize> Default for KernelStackPool<N, BYTES> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessState {
    Vacant,
    Ready,
    Running {
        cpu: usize,
    },
    Blocked,
    Exited {
        code: i64,
    },
    /// Exceção irrecuperável em CPL3. Os detalhes permanecem disponíveis ao
    /// reaper para diagnóstico antes da liberação dos recursos.
    Faulted {
        vector: u8,
        error_code: u64,
        address: u64,
    },
    /// Saiu das filas e está tendo seus recursos liberados fora do lock.
    Reaping {
        code: i64,
    },
    ReapingFaulted {
        vector: u8,
        error_code: u64,
        address: u64,
    },
}

/// Estado de CPU que precisa sobreviver a uma troca cooperativa ou à
/// preempção. `rsp` é a pilha de usuário; o backend x86 guarda separadamente
/// o frame de interrupção e a pilha privilegiada do processo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CpuContext {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub rsp: u64,
    pub rflags: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Process {
    pub id: u64,
    pub parent_id: Option<u64>,
    /// Identificador do processo líder compartilhado pelas threads do mesmo grupo.
    pub thread_group_id: u64,
    /// Alvo aguardado pela thread, até ela consumir o resultado do join.
    pub join_target: Option<u64>,
    join_claimed: bool,
    joinable: bool,
    fixed_cpu: Option<usize>,
    resume_cpu: Option<usize>,
    pub page_table: u64,
    pub entry: u64,
    pub user_stack: u64,
    pub context: CpuContext,
    pub fs_base: u64,
    serial_fds: u8,
    ignores_sigpipe: bool,
    /// Topo da pilha privilegiada exclusiva; 16-byte aligned, endereço virtual.
    pub kernel_stack_top: u64,
    /// Índice da pilha possuída no KernelStackPool; None apenas no modelo host.
    pub kernel_stack_owner: Option<usize>,
    /// Frame de retorno salvo em interrupção (ponteiro virtual no espaço kernel).
    pub interrupt_frame: u64,
    pub state: ProcessState,
    /// Credenciais POSIX por processo; a umask é mutável via `umask(2)`.
    pub security: ProcessSecurityContext,
    /// Descritor dos recursos de execução pertencentes ao processo. O backend
    /// associa o token ao VM e ao teardown antes de publicar a tarefa.
    pub resources: Option<ProcessResourceBundle>,
}

/// Manifesto copiado do bundle de recursos do processo. `slot` indexa o
/// registro de VM do backend; raiz e pilha permitem validar que o registro
/// corresponde à identidade despachada pelo scheduler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessResourceBundle {
    pub slot: u8,
    pub page_table: u64,
    pub user_stack: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessSecurityContext {
    domain: crate::dircache_policy::Domain,
    capabilities: u64,
    pub credentials: Credentials,
    pub supplementary_groups: [u32; 8],
    pub group_count: u8,
    pub umask: u16,
}

impl ProcessSecurityContext {
    pub const ROOT: Self = Self {
        domain: crate::dircache_policy::Domain::Ordinary,
        capabilities: 0,
        credentials: Credentials { uid: 0, gid: 0 },
        supplementary_groups: [0; 8],
        group_count: 0,
        umask: 0o022,
    };

    pub fn new(credentials: Credentials, groups: &[u32], umask: u16) -> Option<Self> {
        if groups.len() > 8 {
            return None;
        }
        let mut supplementary_groups = [0; 8];
        supplementary_groups[..groups.len()].copy_from_slice(groups);
        Some(Self {
            domain: crate::dircache_policy::Domain::Ordinary,
            capabilities: 0,
            credentials,
            supplementary_groups,
            group_count: groups.len() as u8,
            umask: umask & 0o777,
        })
    }

    fn set_executable_domain(&mut self, domain: crate::dircache_policy::Domain) {
        self.domain = domain;
        self.capabilities = match domain {
            crate::dircache_policy::Domain::DirectoryBackend => {
                crate::dircache_policy::CAP_DIRCACHE_REGISTER
            }
            crate::dircache_policy::Domain::Ordinary => 0,
        };
    }

    pub fn permits_dircache_registration(self) -> bool {
        self.domain.permits_registration(self.capabilities)
    }

    /// Applies the domain returned by the trusted ELF loader to a supervised
    /// launch. The domain grants only registration with the dircache broker.
    pub fn supervised_image(mut self, domain: crate::dircache_policy::Domain) -> Self {
        self.set_executable_domain(domain);
        self
    }

    pub fn groups(&self) -> &[u32] {
        &self.supplementary_groups[..self.group_count as usize]
    }

    pub fn access_context(&self) -> crate::vfs::AccessContext<'_> {
        crate::vfs::AccessContext::new(self.credentials, self.groups(), self.umask)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProcessTableStats {
    pub capacity: usize,
    pub active: usize,
    pub ready: usize,
    pub running: usize,
    pub blocked: usize,
    pub exited: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinStatus {
    Blocked,
    Exited(i64),
    Faulted,
}

pub enum CredentialChange<'a> {
    User(u32),
    Group(u32),
    SupplementaryGroups(&'a [u32]),
}

impl Process {
    pub const fn is_thread(&self) -> bool {
        self.id != self.thread_group_id
    }

    pub fn has_serial_fd(&self, fd: u64) -> bool {
        fd < 3 && (self.serial_fds & (1 << fd)) != 0
    }

    const EMPTY: Self = Self {
        id: 0,
        parent_id: None,
        thread_group_id: 0,
        join_target: None,
        join_claimed: false,
        joinable: false,
        fixed_cpu: None,
        resume_cpu: None,
        page_table: 0,
        entry: 0,
        user_stack: 0,
        fs_base: 0,
        serial_fds: 0,
        ignores_sigpipe: false,
        context: CpuContext {
            rax: 0,
            rbx: 0,
            rcx: 0,
            rdx: 0,
            rsi: 0,
            rdi: 0,
            rbp: 0,
            r8: 0,
            r9: 0,
            r10: 0,
            r11: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            rip: 0,
            rsp: 0,
            rflags: 0x202,
        },
        kernel_stack_top: 0,
        kernel_stack_owner: None,
        interrupt_frame: 0,
        state: ProcessState::Vacant,
        security: ProcessSecurityContext::ROOT,
        resources: None,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessError {
    Full,
    InvalidCpu,
    UnknownProcess,
    NotRunnable,
    Duplicate,
    Deadlock,
    /// O processo ainda está em execução numa CPU e precisa sair por preempção.
    Busy,
}

struct State<const P: usize, const C: usize> {
    processes: [Process; P],
    ready_next: [usize; P],
    ready_queued: [bool; P],
    ready_head: [usize; C],
    ready_tail: [usize; C],
    ready_len: [usize; C],
    queue_cursor: usize,
    current_slot: [usize; C],
    prefer_steal: [bool; C],
    vacant_cursor: usize,
    next_id: u64,
}

struct ProcessInit {
    page_table: u64,
    context: CpuContext,
    kernel_stack_top: u64,
    interrupt_frame: u64,
    security: ProcessSecurityContext,
    kernel_stack_owner: Option<usize>,
    thread_parent: Option<u64>,
}

/// Estado serializado pelo mesmo lock para preservar a propriedade exclusiva
/// de um processo mesmo quando duas CPUs chamam o escalonador ao mesmo tempo.
pub struct Scheduler<const P: usize, const C: usize, R = ()> {
    state: IrqSpinLock<State<P, C>>,
    resources: [IrqSpinLock<Option<OwnedProcessResource<R>>>; P],
}

/// The scheduler owns the resource itself. Its execution snapshots carry
/// only the identity and slot, so copying a snapshot cannot copy ownership.
pub struct OwnedProcessResource<R> {
    pub owner: Option<u64>,
    pub value: R,
}

impl<const P: usize, const C: usize, R> Scheduler<P, C, R> {
    pub const fn new() -> Self {
        Self {
            resources: [const { IrqSpinLock::new(None) }; P],
            state: IrqSpinLock::new(State {
                processes: [Process::EMPTY; P],
                ready_next: [P; P],
                ready_queued: [false; P],
                ready_head: [P; C],
                ready_tail: [P; C],
                ready_len: [0; C],
                queue_cursor: 0,
                current_slot: [P; C],
                prefer_steal: [false; C],
                vacant_cursor: 0,
                next_id: 1,
            }),
        }
    }

    /// The resource locks are independent of scheduling so VM operations can
    /// wait for TLB acknowledgements without holding the run-queue lock.
    pub const fn owned_resources(&self) -> &[IrqSpinLock<Option<OwnedProcessResource<R>>>] {
        &self.resources
    }

    pub fn create(
        &self,
        page_table: u64,
        entry: u64,
        user_stack: u64,
    ) -> Result<u64, ProcessError> {
        self.create_with_context(
            page_table,
            CpuContext {
                rip: entry,
                rsp: user_stack,
                rflags: 0x202,
                ..CpuContext::default()
            },
        )
    }

    pub fn create_with_context(
        &self,
        page_table: u64,
        context: CpuContext,
    ) -> Result<u64, ProcessError> {
        self.insert_process(ProcessInit {
            page_table,
            context,
            kernel_stack_top: 0,
            interrupt_frame: 0,
            security: ProcessSecurityContext::ROOT,
            kernel_stack_owner: None,
            thread_parent: None,
        })
    }

    /// Clone o contexto de uma thread no mesmo espaço virtual e grupo POSIX.
    /// `context.rsp` deve apontar para uma pilha user recém-mapeada no grupo.
    pub fn clone_thread<const N: usize, const BYTES: usize>(
        &self,
        pool: &KernelStackPool<N, BYTES>,
        parent_id: u64,
        context: CpuContext,
    ) -> Result<u64, ProcessError> {
        self.create_thread_with_stack_pool(pool, parent_id, context)
    }

    /// Aguarda uma thread do mesmo grupo. Se ela ainda estiver ativa, bloqueia
    /// a thread corrente; o término/fault acorda o waiter e preserva o zombie
    /// até que o resultado seja consumido.
    pub fn join_current(&self, cpu: usize, target_id: u64) -> Result<JoinStatus, ProcessError> {
        let mut state = self.state.lock();
        if cpu >= C {
            return Err(ProcessError::InvalidCpu);
        }
        let waiter_slot = state.current_slot[cpu];
        if waiter_slot >= P {
            return Err(ProcessError::NotRunnable);
        }
        let target_slot = find(&state.processes, target_id).ok_or(ProcessError::UnknownProcess)?;
        if waiter_slot == target_slot
            || state.processes[waiter_slot].thread_group_id
                != state.processes[target_slot].thread_group_id
            || !state.processes[target_slot].joinable
        {
            return Err(ProcessError::NotRunnable);
        }
        if join_would_deadlock(&state.processes, state.processes[waiter_slot].id, target_id) {
            return Err(ProcessError::Deadlock);
        }
        match state.processes[target_slot].state {
            ProcessState::Exited { code } => {
                state.processes[target_slot].join_claimed = false;
                state.processes[target_slot].joinable = false;
                state.processes[waiter_slot].join_target = None;
                Ok(JoinStatus::Exited(code))
            }
            ProcessState::Faulted { .. } => {
                state.processes[target_slot].join_claimed = false;
                state.processes[target_slot].joinable = false;
                state.processes[waiter_slot].join_target = None;
                Ok(JoinStatus::Faulted)
            }
            ProcessState::Ready | ProcessState::Running { .. } | ProcessState::Blocked => {
                if state.processes[target_slot].join_claimed {
                    return Err(ProcessError::Duplicate);
                }
                state.processes[target_slot].join_claimed = true;
                state.processes[waiter_slot].join_target = Some(target_id);
                state.processes[waiter_slot].state = ProcessState::Blocked;
                state.current_slot[cpu] = P;
                Ok(JoinStatus::Blocked)
            }
            _ => Err(ProcessError::NotRunnable),
        }
    }

    pub fn create_with_security_context(
        &self,
        page_table: u64,
        context: CpuContext,
        security: ProcessSecurityContext,
    ) -> Result<u64, ProcessError> {
        self.insert_process(ProcessInit {
            page_table,
            context,
            kernel_stack_top: 0,
            interrupt_frame: 0,
            security,
            kernel_stack_owner: None,
            thread_parent: None,
        })
    }

    /// Registra recursos de execução. Stack e frame podem ser zero apenas no
    /// modelo host; o backend de arquitetura deve exigir ambos antes de dispatch.
    pub fn create_with_execution_resources(
        &self,
        page_table: u64,
        context: CpuContext,
        kernel_stack_top: u64,
        interrupt_frame: u64,
    ) -> Result<u64, ProcessError> {
        self.create_with_execution_resources_and_security(
            page_table,
            context,
            kernel_stack_top,
            interrupt_frame,
            ProcessSecurityContext::ROOT,
        )
    }

    pub fn create_with_execution_resources_and_security(
        &self,
        page_table: u64,
        context: CpuContext,
        kernel_stack_top: u64,
        interrupt_frame: u64,
        security: ProcessSecurityContext,
    ) -> Result<u64, ProcessError> {
        self.create_with_owned_execution_resources(
            page_table,
            context,
            kernel_stack_top,
            interrupt_frame,
            security,
            None,
        )
    }

    fn create_with_owned_execution_resources(
        &self,
        page_table: u64,
        context: CpuContext,
        kernel_stack_top: u64,
        interrupt_frame: u64,
        security: ProcessSecurityContext,
        kernel_stack_owner: Option<usize>,
    ) -> Result<u64, ProcessError> {
        if kernel_stack_top == 0
            || interrupt_frame == 0
            || page_table & 0xfff != 0
            || page_table & !0x000f_ffff_ffff_f000 != 0
            || interrupt_frame & 0xf != 0
        {
            return Err(ProcessError::NotRunnable);
        }
        self.insert_process(ProcessInit {
            page_table,
            context,
            kernel_stack_top,
            interrupt_frame,
            security,
            kernel_stack_owner,
            thread_parent: None,
        })
    }

    /// Consome o token da pilha e registra sua posse no processo.
    pub fn create_with_kernel_stack(
        &self,
        page_table: u64,
        context: CpuContext,
        stack: &KernelStack,
    ) -> Result<u64, ProcessError> {
        let frame = stack
            .top
            .checked_sub(USER_INTERRUPT_FRAME_BYTES)
            .ok_or(ProcessError::NotRunnable)?;
        self.create_with_owned_execution_resources(
            page_table,
            context,
            stack.top,
            frame,
            ProcessSecurityContext::ROOT,
            Some(stack.slot),
        )
    }

    /// Aloca e transfere uma pilha do pool atomicamente; em falha de criação o
    /// slot retorna ao pool, sem deixar recurso órfão.
    pub fn create_with_stack_pool<const N: usize, const BYTES: usize>(
        &self,
        pool: &KernelStackPool<N, BYTES>,
        page_table: u64,
        context: CpuContext,
    ) -> Result<u64, ProcessError> {
        self.create_with_stack_pool_and_security(
            pool,
            page_table,
            context,
            ProcessSecurityContext::ROOT,
        )
    }

    pub fn create_with_stack_pool_and_security<const N: usize, const BYTES: usize>(
        &self,
        pool: &KernelStackPool<N, BYTES>,
        page_table: u64,
        context: CpuContext,
        security: ProcessSecurityContext,
    ) -> Result<u64, ProcessError> {
        self.create_with_stack_pool_inner(pool, page_table, context, security, None, false)
    }

    fn create_with_stack_pool_inner<const N: usize, const BYTES: usize>(
        &self,
        pool: &KernelStackPool<N, BYTES>,
        page_table: u64,
        context: CpuContext,
        security: ProcessSecurityContext,
        initial_cpu: Option<usize>,
        suspended: bool,
    ) -> Result<u64, ProcessError> {
        let stack = pool.allocate().ok_or(ProcessError::Full)?;
        let Some(frame) = stack.top.checked_sub(USER_INTERRUPT_FRAME_BYTES) else {
            let _ = pool.release(stack);
            return Err(ProcessError::NotRunnable);
        };
        match self.insert_process_on_cpu(
            ProcessInit {
                page_table,
                context,
                kernel_stack_top: stack.top,
                interrupt_frame: frame,
                security,
                kernel_stack_owner: Some(stack.slot),
                thread_parent: None,
            },
            initial_cpu,
            suspended,
        ) {
            Ok(id) => Ok(id),
            Err(error) => {
                // A criação rejeitada não publicou o processo; devolve o slot.
                let _ = pool.release(KernelStack {
                    slot: stack.slot,
                    top: stack.top,
                });
                Err(error)
            }
        }
    }

    /// Queues a foreground process on the CPU that owns its kernel continuation.
    /// The caller must prevent dispatch on that CPU until resources are attached.
    pub fn create_foreground_with_stack<const N: usize, const BYTES: usize>(
        &self,
        pool: &KernelStackPool<N, BYTES>,
        cpu: usize,
        page_table: u64,
        context: CpuContext,
        security: ProcessSecurityContext,
    ) -> Result<u64, ProcessError> {
        self.create_with_stack_pool_inner(pool, page_table, context, security, Some(cpu), false)
    }

    /// Allocates an AP-bound task without publishing it to a ready queue.
    /// The launcher prepares its frame and attaches its VM before waking it.
    pub fn create_suspended_with_stack<const N: usize, const BYTES: usize>(
        &self,
        pool: &KernelStackPool<N, BYTES>,
        cpu: usize,
        page_table: u64,
        context: CpuContext,
        security: ProcessSecurityContext,
    ) -> Result<u64, ProcessError> {
        self.create_with_stack_pool_inner(pool, page_table, context, security, Some(cpu), true)
    }

    /// Cria uma thread persistente no grupo de `parent_id`, compartilhando
    /// page table e identidade de segurança, com pilha privilegiada exclusiva.
    /// O chamador deve mapear `context.rsp` como pilha user independente no
    /// mesmo espaço antes de agendar a thread. O frame inicial x86 deve ser
    /// preparado pelo backend de arquitetura depois da criação.
    pub fn create_thread_with_stack_pool<const N: usize, const BYTES: usize>(
        &self,
        pool: &KernelStackPool<N, BYTES>,
        parent_id: u64,
        context: CpuContext,
    ) -> Result<u64, ProcessError> {
        let stack = pool.allocate().ok_or(ProcessError::Full)?;
        let Some(frame) = stack.top.checked_sub(USER_INTERRUPT_FRAME_BYTES) else {
            let _ = pool.release(stack);
            return Err(ProcessError::NotRunnable);
        };
        match self.insert_process(ProcessInit {
            page_table: 0,
            context,
            kernel_stack_top: stack.top,
            interrupt_frame: frame,
            security: ProcessSecurityContext::ROOT,
            kernel_stack_owner: Some(stack.slot),
            thread_parent: Some(parent_id),
        }) {
            Ok(id) => Ok(id),
            Err(error) => {
                let _ = pool.release(stack);
                Err(error)
            }
        }
    }

    fn insert_process(&self, init: ProcessInit) -> Result<u64, ProcessError> {
        self.insert_process_on_cpu(init, None, false)
    }

    fn insert_process_on_cpu(
        &self,
        init: ProcessInit,
        initial_cpu: Option<usize>,
        suspended: bool,
    ) -> Result<u64, ProcessError> {
        if initial_cpu.is_some_and(|cpu| cpu >= C) {
            return Err(ProcessError::InvalidCpu);
        }
        let ProcessInit {
            page_table,
            context,
            kernel_stack_top,
            interrupt_frame,
            security,
            kernel_stack_owner,
            thread_parent,
        } = init;
        if !valid_context(context) {
            return Err(ProcessError::NotRunnable);
        }
        if kernel_stack_top != 0
            && (kernel_stack_top & 0xf != 0 || !is_kernel_address(kernel_stack_top))
        {
            return Err(ProcessError::NotRunnable);
        }
        if interrupt_frame != 0 && !is_kernel_address(interrupt_frame) {
            return Err(ProcessError::NotRunnable);
        }
        let mut s = self.state.lock();
        let slot = vacant_slot(&mut s).ok_or(ProcessError::Full)?;
        if C == 0 || ready_total(&s) >= P {
            return Err(ProcessError::Full);
        }
        let (page_table, security, thread_group_id, joinable) = if let Some(parent_id) =
            thread_parent
        {
            let parent_slot = find(&s.processes, parent_id).ok_or(ProcessError::UnknownProcess)?;
            let parent = s.processes[parent_slot];
            if !matches!(
                parent.state,
                ProcessState::Ready | ProcessState::Running { .. } | ProcessState::Blocked
            ) {
                return Err(ProcessError::NotRunnable);
            }
            let leader_slot =
                find(&s.processes, parent.thread_group_id).ok_or(ProcessError::UnknownProcess)?;
            if !matches!(
                s.processes[leader_slot].state,
                ProcessState::Ready | ProcessState::Running { .. } | ProcessState::Blocked
            ) {
                return Err(ProcessError::NotRunnable);
            }
            (
                parent.page_table,
                parent.security,
                parent.thread_group_id,
                true,
            )
        } else {
            (page_table, security, 0, false)
        };
        if page_table == 0 {
            return Err(ProcessError::NotRunnable);
        }
        let id = s.next_id;
        s.next_id = s.next_id.checked_add(1).ok_or(ProcessError::Full)?;
        s.processes[slot] = Process {
            id,
            parent_id: None,
            thread_group_id: if thread_group_id == 0 {
                id
            } else {
                thread_group_id
            },
            join_target: None,
            join_claimed: false,
            joinable,
            fixed_cpu: initial_cpu,
            resume_cpu: None,
            page_table,
            entry: context.rip,
            user_stack: context.rsp,
            fs_base: 0,
            serial_fds: if suspended { 0 } else { 0b111 },
            ignores_sigpipe: false,
            context,
            kernel_stack_top,
            kernel_stack_owner,
            interrupt_frame,
            state: if suspended {
                ProcessState::Blocked
            } else {
                ProcessState::Ready
            },
            security,
            resources: None,
        };
        if suspended {
            return Ok(id);
        }
        let cpu = initial_cpu
            .or_else(|| least_loaded_queue(&mut s))
            .ok_or(ProcessError::InvalidCpu)?;
        push_slot(&mut s, slot, cpu)?;
        Ok(id)
    }

    /// Salva o contexto interrompido do processo atual e escolhe o próximo.
    /// O chamador deve bloquear interrupções locais durante captura/aplicação.
    pub fn preempt(
        &self,
        cpu: usize,
        context: CpuContext,
    ) -> Result<Option<Process>, ProcessError> {
        if !valid_context(context) {
            return Err(ProcessError::NotRunnable);
        }
        {
            let mut s = self.state.lock();
            if cpu >= C {
                return Err(ProcessError::InvalidCpu);
            }
            let slot = s.current_slot[cpu];
            if slot != P {
                if slot >= P {
                    return Err(ProcessError::NotRunnable);
                }
                if !matches!(s.processes[slot].state, ProcessState::Running { cpu: c } if c == cpu)
                {
                    return Err(ProcessError::NotRunnable);
                }
                s.processes[slot].context = context;
                s.processes[slot].entry = context.rip;
                s.processes[slot].user_stack = context.rsp;
            }
        }
        self.schedule(cpu)
    }

    /// Salva o frame completo na pilha kernel corrente e despacha o próximo.
    /// O endereço fica válido até o processo voltar a executar; cada processo
    /// precisa possuir uma pilha kernel mapeada e não reutilizada.
    pub fn preempt_frame(
        &self,
        cpu: usize,
        frame_address: u64,
    ) -> Result<Option<Process>, ProcessError> {
        if !is_kernel_address(frame_address) || frame_address & 0xf != 0 {
            return Err(ProcessError::NotRunnable);
        }
        self.preempt_frame_inner(cpu, frame_address, true)
    }

    /// Reagenda o processo interrompido na fila local, sem roubo entre CPUs.
    pub fn preempt_frame_local(
        &self,
        cpu: usize,
        frame_address: u64,
    ) -> Result<Option<Process>, ProcessError> {
        if !is_kernel_address(frame_address) || frame_address & 0xf != 0 {
            return Err(ProcessError::NotRunnable);
        }
        self.preempt_frame_inner(cpu, frame_address, false)
    }

    fn preempt_frame_inner(
        &self,
        cpu: usize,
        frame_address: u64,
        allow_steal: bool,
    ) -> Result<Option<Process>, ProcessError> {
        let mut state = self.state.lock();
        let slot = *state
            .current_slot
            .get(cpu)
            .ok_or(ProcessError::InvalidCpu)?;
        if slot == P && !allow_steal {
            return Err(ProcessError::NotRunnable);
        }
        if slot != P {
            let process = state
                .processes
                .get_mut(slot)
                .ok_or(ProcessError::NotRunnable)?;
            if process.state != (ProcessState::Running { cpu }) {
                return Err(ProcessError::NotRunnable);
            }
            process.interrupt_frame = frame_address;
        }
        Self::schedule_locked(&mut state, cpu, allow_steal)
    }

    /// Despacha um processo e devolve o processo anteriormente ativo à fila.
    /// Nenhum estado de CPU é salvo por esta função.
    pub fn schedule(&self, cpu: usize) -> Result<Option<Process>, ProcessError> {
        self.schedule_inner(cpu, true)
    }

    /// Despacha somente a fila local, preservando ownership do frame e pilha
    /// kernel durante preempção de processos em CPUs secundárias.
    pub fn schedule_local(&self, cpu: usize) -> Result<Option<Process>, ProcessError> {
        self.schedule_inner(cpu, false)
    }

    fn schedule_inner(
        &self,
        cpu: usize,
        allow_steal: bool,
    ) -> Result<Option<Process>, ProcessError> {
        Self::schedule_locked(&mut self.state.lock(), cpu, allow_steal)
    }

    fn schedule_locked(
        s: &mut State<P, C>,
        cpu: usize,
        allow_steal: bool,
    ) -> Result<Option<Process>, ProcessError> {
        if cpu >= C {
            return Err(ProcessError::InvalidCpu);
        }
        if s.current_slot[cpu] != P {
            let slot = s.current_slot[cpu];
            if slot >= P {
                return Err(ProcessError::NotRunnable);
            }
            if matches!(s.processes[slot].state, ProcessState::Running { .. }) {
                s.processes[slot].state = ProcessState::Ready;
                push_slot(s, slot, cpu)?;
                s.prefer_steal[cpu] = allow_steal;
            }
            s.current_slot[cpu] = P;
        }
        while let Some(queue) = if allow_steal {
            next_ready_queue(s, cpu, s.prefer_steal[cpu])
        } else {
            (s.ready_len[cpu] != 0).then_some(cpu)
        } {
            let queued = s.ready_head[queue];
            if queued >= P || !s.ready_queued[queued] {
                return Err(ProcessError::NotRunnable);
            }
            s.ready_head[queue] = s.ready_next[queued];
            s.ready_next[queued] = P;
            s.ready_queued[queued] = false;
            s.ready_len[queue] -= 1;
            if s.ready_len[queue] == 0 {
                s.ready_tail[queue] = P;
            }
            if s.processes[queued].state != ProcessState::Ready {
                continue;
            }
            s.processes[queued].state = ProcessState::Running { cpu };
            s.processes[queued].resume_cpu = None;
            s.current_slot[cpu] = queued;
            s.prefer_steal[cpu] = false;
            return Ok(Some(s.processes[queued]));
        }
        Ok(None)
    }

    pub fn block(&self, cpu: usize) -> Result<(), ProcessError> {
        self.stop_current(cpu, ProcessState::Blocked)
    }

    /// Saves a user frame and parks the current task. Its first dispatch after
    /// waking stays on this CPU, which still owns the kernel stack.
    pub fn block_frame(&self, cpu: usize, frame: u64) -> Result<(), ProcessError> {
        let mut s = self.state.lock();
        let slot = *s.current_slot.get(cpu).ok_or(ProcessError::InvalidCpu)?;
        if slot >= P || s.processes[slot].state != (ProcessState::Running { cpu }) {
            return Err(ProcessError::NotRunnable);
        }
        s.processes[slot].interrupt_frame = frame;
        s.processes[slot].resume_cpu = Some(cpu);
        s.processes[slot].state = ProcessState::Blocked;
        s.current_slot[cpu] = P;
        Ok(())
    }

    pub fn wake(&self, id: u64) -> Result<(), ProcessError> {
        let mut s = self.state.lock();
        let slot = find(&s.processes, id).ok_or(ProcessError::UnknownProcess)?;
        if s.processes[slot].state != ProcessState::Blocked {
            return Err(ProcessError::NotRunnable);
        }
        if C == 0 || ready_total(&s) >= P {
            return Err(ProcessError::Full);
        }
        s.processes[slot].state = ProcessState::Ready;
        let cpu = s.processes[slot]
            .resume_cpu
            .or(s.processes[slot].fixed_cpu)
            .or_else(|| least_loaded_queue(&mut s))
            .ok_or(ProcessError::InvalidCpu)?;
        push_slot(&mut s, slot, cpu)
    }

    pub fn exit(&self, cpu: usize, code: i64) -> Result<(), ProcessError> {
        self.stop_current(cpu, ProcessState::Exited { code })
    }

    /// Solicita o encerramento de uma tarefa que não está executando.
    /// Tarefas prontas são removidas da fila antes de mudar de estado; tarefas
    /// bloqueadas passam diretamente a Exited. Uma tarefa Running exige
    /// cooperação da CPU proprietária e retorna Busy, evitando liberar recursos
    /// enquanto o contexto ainda pode estar usando sua pilha ou page table.
    /// O chamador deve executar `reap` depois de destruir os recursos de VM.
    pub fn destroy(&self, id: u64, code: i64) -> Result<(), ProcessError> {
        let mut state = self.state.lock();
        let slot = find(&state.processes, id).ok_or(ProcessError::UnknownProcess)?;
        match state.processes[slot].state {
            ProcessState::Ready => {
                unlink_ready_slot(&mut state, slot)?;
                state.processes[slot].state = ProcessState::Exited { code };
                cancel_join_claim(&mut state, slot);
                wake_joiners(&mut state, id)?;
                Ok(())
            }
            ProcessState::Blocked => {
                state.processes[slot].state = ProcessState::Exited { code };
                cancel_join_claim(&mut state, slot);
                wake_joiners(&mut state, id)?;
                Ok(())
            }
            ProcessState::Running { .. } => Err(ProcessError::Busy),
            _ => Err(ProcessError::NotRunnable),
        }
    }

    /// Confina uma exceção de usuário ao processo corrente. O chamador ainda
    /// precisa despachar outra tarefa e, fora da pilha da vítima, chamar reap.
    pub fn fault(
        &self,
        cpu: usize,
        vector: u8,
        error_code: u64,
        address: u64,
    ) -> Result<(), ProcessError> {
        self.stop_current(
            cpu,
            ProcessState::Faulted {
                vector,
                error_code,
                address,
            },
        )
    }
    pub fn process(&self, id: u64) -> Option<Process> {
        let s = self.state.lock();
        find(&s.processes, id).map(|i| s.processes[i])
    }

    /// Returns a snapshot of terminal tasks that have no join or live group
    /// members preventing reclamation. Reap still validates each candidate.
    pub fn reapable_ids(&self) -> [Option<u64>; P] {
        let state = self.state.lock();
        core::array::from_fn(|slot| {
            let process = state.processes[slot];
            (matches!(
                process.state,
                ProcessState::Exited { .. } | ProcessState::Faulted { .. }
            ) && !process.join_claimed
                && !process.joinable
                && (process.is_thread()
                    || !state.processes.iter().any(|member| {
                        member.id != process.id
                            && member.thread_group_id == process.id
                            && member.state != ProcessState::Vacant
                    })))
            .then_some(process.id)
        })
    }

    /// Report one waitable child, or None while matching children are alive.
    pub fn wait_child(
        &self,
        parent: u64,
        target: Option<u64>,
    ) -> Result<Option<Process>, ProcessError> {
        let state = self.state.lock();
        let mut children = state
            .processes
            .iter()
            .filter(|task| task.parent_id == Some(parent) && target.is_none_or(|id| id == task.id))
            .peekable();
        if children.peek().is_none() {
            return Err(ProcessError::UnknownProcess);
        }
        Ok(children
            .find(|task| {
                matches!(
                    task.state,
                    ProcessState::Exited { .. } | ProcessState::Faulted { .. }
                )
            })
            .copied())
    }

    /// Associa ao processo o token do bundle mantido pelo backend de memória.
    /// A associação é permitida somente enquanto a tarefa ainda está pronta,
    /// antes de qualquer CPU poder despachá-la.
    pub fn attach_resource_bundle(
        &self,
        id: u64,
        bundle: ProcessResourceBundle,
    ) -> Result<(), ProcessError> {
        if bundle.page_table == 0 || bundle.page_table & 0xfff != 0 || bundle.user_stack == 0 {
            return Err(ProcessError::NotRunnable);
        }
        let mut state = self.state.lock();
        let slot = find(&state.processes, id).ok_or(ProcessError::UnknownProcess)?;
        if state.processes.iter().any(|other| {
            other.id != id
                && other.state != ProcessState::Vacant
                && other
                    .resources
                    .is_some_and(|resources| resources.slot == bundle.slot)
        }) {
            return Err(ProcessError::Duplicate);
        }
        let process = &mut state.processes[slot];
        if !(process.state == ProcessState::Ready
            || (process.state == ProcessState::Blocked && process.resume_cpu.is_none()))
            || process.resources.is_some()
            || process.page_table != bundle.page_table
            || process.user_stack != bundle.user_stack
        {
            return Err(ProcessError::NotRunnable);
        }
        process.resources = Some(bundle);
        Ok(())
    }

    /// Substitui a imagem do processo corrente mantendo PID, credenciais e
    /// identidade de thread. Só aceita processo sem irmãos ativos para que o
    /// address space antigo possa ser retirado após a troca de CR3.
    pub fn replace_current_image(
        &self,
        cpu: usize,
        id: u64,
        bundle: ProcessResourceBundle,
        instruction_pointer: u64,
        stack_pointer: u64,
        domain: crate::dircache_policy::Domain,
    ) -> Result<ProcessResourceBundle, ProcessError> {
        if cpu >= C
            || bundle.page_table == 0
            || bundle.page_table & 0xfff != 0
            || bundle.user_stack != stack_pointer
            || !valid_context(CpuContext {
                rip: instruction_pointer,
                rsp: stack_pointer,
                rflags: 0x202,
                ..CpuContext::default()
            })
        {
            return Err(ProcessError::NotRunnable);
        }
        let mut state = self.state.lock();
        let slot = find(&state.processes, id).ok_or(ProcessError::UnknownProcess)?;
        let process = state.processes[slot];
        if state.current_slot[cpu] != slot
            || process.state != (ProcessState::Running { cpu })
            || process.id != process.thread_group_id
            || process.resources.is_none()
        {
            return Err(ProcessError::NotRunnable);
        }
        if state.processes.iter().any(|other| {
            other.id != id
                && other.state != ProcessState::Vacant
                && other.thread_group_id == process.thread_group_id
        }) {
            return Err(ProcessError::Busy);
        }
        if state.processes.iter().any(|other| {
            other.id != id
                && other.state != ProcessState::Vacant
                && other
                    .resources
                    .is_some_and(|resources| resources.slot == bundle.slot)
        }) {
            return Err(ProcessError::Duplicate);
        }
        let old = process.resources.ok_or(ProcessError::NotRunnable)?;
        let process = &mut state.processes[slot];
        let domain = if process.security.credentials.uid == 0 {
            domain
        } else {
            crate::dircache_policy::Domain::Ordinary
        };
        process.security.set_executable_domain(domain);
        process.fs_base = 0;
        process.page_table = bundle.page_table;
        process.entry = instruction_pointer;
        process.user_stack = stack_pointer;
        process.context = CpuContext {
            rip: instruction_pointer,
            rsp: stack_pointer,
            rflags: 0x202,
            ..CpuContext::default()
        };
        process.resources = Some(bundle);
        Ok(old)
    }

    /// Fixa uma tarefa pronta em uma fila de CPU sem alterar seu frame ou
    /// ownership. Tarefas em execução só podem migrar pelo caminho de preempção.
    pub fn pin_ready_to_cpu(&self, id: u64, cpu: usize) -> Result<(), ProcessError> {
        let mut state = self.state.lock();
        if cpu >= C {
            return Err(ProcessError::InvalidCpu);
        }
        let slot = find(&state.processes, id).ok_or(ProcessError::UnknownProcess)?;
        if state.processes[slot].state != ProcessState::Ready || !state.ready_queued[slot] {
            return Err(ProcessError::NotRunnable);
        }
        if state.processes[slot]
            .resume_cpu
            .or(state.processes[slot].fixed_cpu)
            .is_some_and(|owner| owner != cpu)
        {
            return Err(ProcessError::NotRunnable);
        }
        let source = (0..C)
            .find(|&queue| {
                let mut cursor = state.ready_head[queue];
                while cursor < P {
                    if cursor == slot {
                        return true;
                    }
                    cursor = state.ready_next[cursor];
                }
                false
            })
            .ok_or(ProcessError::NotRunnable)?;
        if source == cpu {
            return Ok(());
        }
        let mut previous = P;
        let mut cursor = state.ready_head[source];
        while cursor < P && cursor != slot {
            previous = cursor;
            cursor = state.ready_next[cursor];
        }
        if cursor != slot {
            return Err(ProcessError::NotRunnable);
        }
        let next = state.ready_next[slot];
        if previous == P {
            state.ready_head[source] = next;
        } else {
            state.ready_next[previous] = next;
        }
        if state.ready_tail[source] == slot {
            state.ready_tail[source] = previous;
        }
        state.ready_len[source] -= 1;
        state.ready_next[slot] = P;
        state.ready_queued[slot] = false;
        push_slot(&mut state, slot, cpu)
    }

    /// Updates the TLS base only for the task running on this CPU.
    pub fn set_current_fs_base(&self, cpu: usize, id: u64, base: u64) -> Result<(), ProcessError> {
        let mut state = self.state.lock();
        let slot = *state
            .current_slot
            .get(cpu)
            .ok_or(ProcessError::InvalidCpu)?;
        let process = state
            .processes
            .get_mut(slot)
            .ok_or(ProcessError::NotRunnable)?;
        if process.id != id || process.state != (ProcessState::Running { cpu }) {
            return Err(ProcessError::NotRunnable);
        }
        process.fs_base = base;
        Ok(())
    }

    pub fn change_current_credentials(
        &self,
        cpu: usize,
        id: u64,
        change: CredentialChange<'_>,
    ) -> Result<(), u16> {
        use crate::linux_abi::{EBUSY, EINVAL, EPERM, ESRCH};
        let mut state = self.state.lock();
        let slot = *state.current_slot.get(cpu).ok_or(ESRCH)?;
        let task = state.processes.get(slot).ok_or(ESRCH)?;
        if task.id != id || task.state != (ProcessState::Running { cpu }) {
            return Err(ESRCH);
        }
        let mut security = task.security;
        if security.credentials.uid != 0 {
            return match change {
                CredentialChange::User(uid) if uid == security.credentials.uid => Ok(()),
                CredentialChange::Group(gid) if gid == security.credentials.gid => Ok(()),
                _ => Err(EPERM),
            };
        }
        if task.is_thread()
            || state
                .processes
                .iter()
                .any(|other| other.is_thread() && other.thread_group_id == id)
        {
            return Err(EBUSY);
        }
        match change {
            CredentialChange::User(uid) => {
                if uid != 0 {
                    security.set_executable_domain(crate::dircache_policy::Domain::Ordinary);
                }
                security.credentials.uid = uid;
            }
            CredentialChange::Group(gid) => security.credentials.gid = gid,
            CredentialChange::SupplementaryGroups(groups) => {
                if groups.len() > security.supplementary_groups.len() {
                    return Err(EINVAL);
                }
                security.supplementary_groups.fill(0);
                security.supplementary_groups[..groups.len()].copy_from_slice(groups);
                security.group_count = groups.len() as u8;
            }
        }
        state.processes[slot].security = security;
        Ok(())
    }

    pub fn can_fork(&self, parent: u64) -> bool {
        let state = self.state.lock();
        find(&state.processes, parent).is_some_and(|slot| {
            let task = state.processes[slot];
            !task.is_thread() && task.fixed_cpu.is_some()
        }) && !state
            .processes
            .iter()
            .any(|p| p.thread_group_id == parent && p.is_thread())
    }

    pub fn prepare_fork_child(&self, cpu: usize, child: u64) -> Result<(), ProcessError> {
        let mut state = self.state.lock();
        let parent_slot = *state
            .current_slot
            .get(cpu)
            .ok_or(ProcessError::InvalidCpu)?;
        let parent = *state
            .processes
            .get(parent_slot)
            .ok_or(ProcessError::NotRunnable)?;
        let child_slot = find(&state.processes, child).ok_or(ProcessError::UnknownProcess)?;
        if parent.is_thread()
            || parent.fixed_cpu != Some(cpu)
            || state
                .processes
                .iter()
                .any(|p| p.thread_group_id == parent.id && p.is_thread())
            || state.processes[child_slot].state != ProcessState::Blocked
            || state.processes[child_slot].fixed_cpu != Some(cpu)
        {
            return Err(ProcessError::NotRunnable);
        }
        let child = &mut state.processes[child_slot];
        child.parent_id = Some(parent.id);
        child.fs_base = parent.fs_base;
        child.serial_fds = parent.serial_fds;
        child.ignores_sigpipe = parent.ignores_sigpipe;
        Ok(())
    }

    pub fn ignores_sigpipe(&self, thread_group_id: u64) -> bool {
        let state = self.state.lock();
        find(&state.processes, thread_group_id)
            .is_some_and(|slot| state.processes[slot].ignores_sigpipe)
    }

    pub fn set_sigpipe_ignored(
        &self,
        thread_group_id: u64,
        ignored: bool,
    ) -> Result<(), ProcessError> {
        let mut state = self.state.lock();
        let leader = find(&state.processes, thread_group_id).ok_or(ProcessError::UnknownProcess)?;
        state.processes[leader].ignores_sigpipe = ignored;
        Ok(())
    }

    /// Connects the console to a suspended task before its initial dispatch.
    pub fn attach_initial_serial_fds(&self, id: u64) -> Result<(), ProcessError> {
        let mut state = self.state.lock();
        let slot = find(&state.processes, id).ok_or(ProcessError::UnknownProcess)?;
        let process = &mut state.processes[slot];
        if process.state != ProcessState::Blocked || process.resume_cpu.is_some() {
            return Err(ProcessError::NotRunnable);
        }
        process.serial_fds = 0b111;
        Ok(())
    }

    pub fn close_serial_fd(&self, cpu: usize, id: u64, fd: u64) -> Result<(), ProcessError> {
        let mut state = self.state.lock();
        let slot = *state
            .current_slot
            .get(cpu)
            .ok_or(ProcessError::InvalidCpu)?;
        let process = state
            .processes
            .get_mut(slot)
            .ok_or(ProcessError::NotRunnable)?;
        if process.id != id
            || process.state != (ProcessState::Running { cpu })
            || !process.has_serial_fd(fd)
        {
            return Err(ProcessError::NotRunnable);
        }
        process.serial_fds &= !(1 << fd);
        Ok(())
    }

    pub fn current(&self, cpu: usize) -> Option<Process> {
        let s = self.state.lock();
        if cpu >= C {
            return None;
        }
        let slot = s.current_slot[cpu];
        if slot < P {
            Some(s.processes[slot])
        } else {
            None
        }
    }

    /// Snapshot para diagnóstico e controle de carga; varre a tabela e não
    /// deve ser chamado pelo caminho de IRQ/timer.
    pub fn stats(&self) -> ProcessTableStats {
        let state = self.state.lock();
        let mut stats = ProcessTableStats {
            capacity: P,
            ready: ready_total(&state),
            ..ProcessTableStats::default()
        };
        for process in &state.processes {
            match process.state {
                ProcessState::Vacant => {}
                ProcessState::Ready => stats.active += 1,
                ProcessState::Running { .. } => {
                    stats.active += 1;
                    stats.running += 1;
                }
                ProcessState::Blocked => {
                    stats.active += 1;
                    stats.blocked += 1;
                }
                ProcessState::Exited { .. } => {
                    stats.active += 1;
                    stats.exited += 1;
                }
                ProcessState::Faulted { .. } => {
                    stats.active += 1;
                    stats.exited += 1;
                }
                ProcessState::Reaping { .. } => stats.active += 1,
                ProcessState::ReapingFaulted { .. } => stats.active += 1,
            }
        }
        stats
    }

    /// Reaper: retira processo encerrado e devolve sua pilha ao pool. Só pode
    /// ser chamado por contexto que já não esteja executando nessa pilha.
    pub fn reap<const N: usize, const BYTES: usize>(
        &self,
        id: u64,
        pool: &KernelStackPool<N, BYTES>,
    ) -> Result<(), ProcessError> {
        self.reap_with(id, pool, |_| Ok::<(), ProcessError>(()))
            .map_err(|error| match error {
                ReapError::Process(error) | ReapError::Cleanup(error) => error,
            })
    }

    /// Recolhe todos os recursos associados pelo chamador a um processo.
    /// O processo é marcado Reaping antes de soltar o lock; o callback pode
    /// destruir espaços de usuário e outros recursos sem bloquear o scheduler.
    /// Se o callback falhar, o estado Exited é restaurado para permitir retry.
    pub fn reap_with<const N: usize, const BYTES: usize, E>(
        &self,
        id: u64,
        pool: &KernelStackPool<N, BYTES>,
        cleanup: impl FnOnce(Process) -> Result<(), E>,
    ) -> Result<(), ReapError<E>> {
        let (process, previous_state) = {
            let mut state = self.state.lock();
            let slot = find(&state.processes, id)
                .ok_or(ReapError::Process(ProcessError::UnknownProcess))?;
            let previous_state = state.processes[slot].state;
            let reaping_state = match previous_state {
                ProcessState::Exited { code } => ProcessState::Reaping { code },
                ProcessState::Faulted {
                    vector,
                    error_code,
                    address,
                } => ProcessState::ReapingFaulted {
                    vector,
                    error_code,
                    address,
                },
                _ => return Err(ReapError::Process(ProcessError::NotRunnable)),
            };
            if state.ready_queued[slot] || state.current_slot.contains(&slot) {
                return Err(ReapError::Process(ProcessError::NotRunnable));
            }
            if state.processes[slot].join_claimed || state.processes[slot].joinable {
                return Err(ReapError::Process(ProcessError::Busy));
            }
            if state.processes[slot].id == state.processes[slot].thread_group_id
                && state.processes.iter().any(|member| {
                    member.id != state.processes[slot].id
                        && member.thread_group_id == state.processes[slot].thread_group_id
                        && member.state != ProcessState::Vacant
                })
            {
                return Err(ReapError::Process(ProcessError::Busy));
            }
            state.processes[slot].state = reaping_state;
            (state.processes[slot], previous_state)
        };

        if let Err(error) = cleanup(process) {
            let mut state = self.state.lock();
            if let Some(slot) = find(&state.processes, id) {
                if matches!(
                    state.processes[slot].state,
                    ProcessState::Reaping { .. } | ProcessState::ReapingFaulted { .. }
                ) {
                    state.processes[slot].state = previous_state;
                }
            }
            return Err(ReapError::Cleanup(error));
        }

        let mut state = self.state.lock();
        let slot =
            find(&state.processes, id).ok_or(ReapError::Process(ProcessError::UnknownProcess))?;
        if !matches!(
            state.processes[slot].state,
            ProcessState::Reaping { .. } | ProcessState::ReapingFaulted { .. }
        ) {
            return Err(ReapError::Process(ProcessError::NotRunnable));
        }
        if state.ready_queued[slot] || state.current_slot.contains(&slot) {
            return Err(ReapError::Process(ProcessError::NotRunnable));
        }
        if state.processes[slot].join_claimed || state.processes[slot].joinable {
            return Err(ReapError::Process(ProcessError::Busy));
        }
        if state.processes[slot].id == state.processes[slot].thread_group_id
            && state.processes.iter().any(|member| {
                member.id != state.processes[slot].id
                    && member.thread_group_id == state.processes[slot].thread_group_id
                    && member.state != ProcessState::Vacant
            })
        {
            return Err(ReapError::Process(ProcessError::Busy));
        }
        if let Some(owner) = state.processes[slot].kernel_stack_owner {
            let top = state.processes[slot].kernel_stack_top;
            pool.release_slot(owner, top).map_err(ReapError::Process)?;
        }
        state.processes[slot] = Process::EMPTY;
        state.vacant_cursor = slot;
        Ok(())
    }

    fn stop_current(&self, cpu: usize, state: ProcessState) -> Result<(), ProcessError> {
        let mut s = self.state.lock();
        if cpu >= C {
            return Err(ProcessError::InvalidCpu);
        }
        let slot = s.current_slot[cpu];
        if slot == P {
            return Err(ProcessError::NotRunnable);
        }
        if slot >= P {
            return Err(ProcessError::NotRunnable);
        }
        cancel_join_claim(&mut s, slot);
        s.processes[slot].state = state;
        s.current_slot[cpu] = P;
        s.prefer_steal[cpu] = true;
        match state {
            ProcessState::Exited { .. } | ProcessState::Faulted { .. } => {
                let id = s.processes[slot].id;
                wake_joiners(&mut s, id)?;
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReapError<E> {
    Process(ProcessError),
    Cleanup(E),
}

const fn is_user_address(address: u64) -> bool {
    address >= 0x1000 && address < 0x0000_8000_0000_0000
}

const fn is_kernel_address(address: u64) -> bool {
    address >= 0xffff_8000_0000_0000
}

const fn valid_context(context: CpuContext) -> bool {
    is_user_address(context.rip)
        && is_user_address(context.rsp)
        && context.rflags & 2 != 0
        && context.rflags & (3 << 12) == 0
        && context.rflags & (1 << 14) == 0
        && context.rflags & (1 << 17) == 0
}
impl<const P: usize, const C: usize, R> Default for Scheduler<P, C, R> {
    fn default() -> Self {
        Self::new()
    }
}

fn find<const P: usize>(ps: &[Process; P], id: u64) -> Option<usize> {
    ps.iter()
        .position(|p| p.id == id && p.state != ProcessState::Vacant)
}
fn vacant_slot<const P: usize, const C: usize>(s: &mut State<P, C>) -> Option<usize> {
    if P == 0 {
        return None;
    }
    for offset in 0..P {
        let slot = (s.vacant_cursor + offset) % P;
        if s.processes[slot].state == ProcessState::Vacant {
            s.vacant_cursor = (slot + 1) % P;
            return Some(slot);
        }
    }
    None
}
fn push_slot<const P: usize, const C: usize>(
    s: &mut State<P, C>,
    slot: usize,
    cpu: usize,
) -> Result<(), ProcessError> {
    if slot >= P {
        return Err(ProcessError::UnknownProcess);
    }
    if cpu >= C {
        return Err(ProcessError::InvalidCpu);
    }
    if s.processes[slot].state != ProcessState::Ready {
        return Err(ProcessError::NotRunnable);
    }
    if s.ready_queued[slot] {
        return Err(ProcessError::Duplicate);
    }
    if ready_total(s) >= P {
        return Err(ProcessError::Full);
    }
    if s.ready_head[cpu] == P {
        s.ready_head[cpu] = slot;
    } else {
        if s.ready_tail[cpu] >= P {
            return Err(ProcessError::NotRunnable);
        }
        s.ready_next[s.ready_tail[cpu]] = slot;
    }
    s.ready_next[slot] = P;
    s.ready_queued[slot] = true;
    s.ready_tail[cpu] = slot;
    s.ready_len[cpu] += 1;
    Ok(())
}

fn unlink_ready_slot<const P: usize, const C: usize>(
    state: &mut State<P, C>,
    target: usize,
) -> Result<(), ProcessError> {
    if target >= P || !state.ready_queued[target] {
        return Err(ProcessError::NotRunnable);
    }
    for cpu in 0..C {
        let mut previous = P;
        let mut current = state.ready_head[cpu];
        while current < P {
            if current == target {
                let next = state.ready_next[current];
                if previous == P {
                    state.ready_head[cpu] = next;
                } else {
                    state.ready_next[previous] = next;
                }
                if state.ready_tail[cpu] == current {
                    state.ready_tail[cpu] = previous;
                }
                state.ready_next[current] = P;
                state.ready_queued[current] = false;
                state.ready_len[cpu] = state.ready_len[cpu]
                    .checked_sub(1)
                    .ok_or(ProcessError::NotRunnable)?;
                return Ok(());
            }
            previous = current;
            current = state.ready_next[current];
        }
    }
    Err(ProcessError::NotRunnable)
}

fn cancel_join_claim<const P: usize, const C: usize>(state: &mut State<P, C>, slot: usize) {
    let target_id = state.processes[slot].join_target.take();
    if let Some(target_id) = target_id {
        if let Some(target_slot) = find(&state.processes, target_id) {
            state.processes[target_slot].join_claimed = false;
        }
    }
}

fn join_would_deadlock<const P: usize>(
    processes: &[Process; P],
    waiter_id: u64,
    target_id: u64,
) -> bool {
    let mut cursor = target_id;
    for _ in 0..P {
        if cursor == waiter_id {
            return true;
        }
        let Some(slot) = find(processes, cursor) else {
            return false;
        };
        let Some(next) = processes[slot].join_target else {
            return false;
        };
        cursor = next;
    }
    // A cycle not containing the new waiter already exists; reject joining it.
    true
}

fn wake_joiners<const P: usize, const C: usize>(
    state: &mut State<P, C>,
    target_id: u64,
) -> Result<(), ProcessError> {
    for slot in 0..P {
        if state.processes[slot].join_target != Some(target_id)
            || state.processes[slot].state != ProcessState::Blocked
        {
            continue;
        }
        if ready_total(state) >= P {
            return Err(ProcessError::Full);
        }
        state.processes[slot].state = ProcessState::Ready;
        let cpu = state.processes[slot]
            .fixed_cpu
            .or_else(|| least_loaded_queue(state))
            .ok_or(ProcessError::InvalidCpu)?;
        push_slot(state, slot, cpu)?;
    }
    Ok(())
}

fn ready_total<const P: usize, const C: usize>(s: &State<P, C>) -> usize {
    s.ready_len.iter().copied().sum()
}

fn least_loaded_queue<const P: usize, const C: usize>(s: &mut State<P, C>) -> Option<usize> {
    if C == 0 {
        return None;
    }
    let mut chosen = s.queue_cursor % C;
    for offset in 1..C {
        let candidate = (s.queue_cursor + offset) % C;
        if s.ready_len[candidate] < s.ready_len[chosen] {
            chosen = candidate;
        }
    }
    s.queue_cursor = (chosen + 1) % C;
    Some(chosen)
}

fn next_ready_queue<const P: usize, const C: usize>(
    s: &State<P, C>,
    cpu: usize,
    prefer_steal: bool,
) -> Option<usize> {
    if cpu >= C {
        return None;
    }
    let local = (s.ready_len[cpu] != 0).then_some(cpu);
    let steal = || {
        (1..C).map(|offset| (cpu + offset) % C).find(|&queue| {
            let head = s.ready_head[queue];
            head < P
                && s.processes[head].fixed_cpu.is_none()
                && s.processes[head].resume_cpu.is_none()
        })
    };
    if prefer_steal {
        steal().or(local)
    } else {
        local.or_else(steal)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reaper_barrier_requires_each_cpu_ack_and_rejects_stale_epochs() {
        let barrier = ReapBarrier::<4>::new();
        let epoch = barrier.begin().expect("reaper epoch must start");

        assert!(!barrier.complete(epoch, 0b1010));
        assert!(!barrier.acknowledge(4, epoch));
        assert!(!barrier.acknowledge(1, epoch + 1));
        assert!(barrier.acknowledge(1, epoch));
        assert!(!barrier.complete(epoch, 0b1010));
        assert!(barrier.acknowledge(3, epoch));
        assert!(barrier.complete(epoch, 0b1010));
        assert!(barrier.release(epoch));
        assert!(!barrier.acknowledge(1, epoch));
        assert!(!barrier.complete(epoch, 0b1010));
    }

    #[test]
    fn schedules_round_robin_across_cpus_without_duplicate_running() {
        let q = Scheduler::<4, 2>::new();
        let a = q.create(0x1000, 0x400000, 0x800000).unwrap();
        let b = q.create(0x2000, 0x500000, 0x900000).unwrap();
        assert_eq!(q.schedule(0).unwrap().unwrap().id, a);
        assert_eq!(q.schedule(1).unwrap().unwrap().id, b);
        assert_eq!(q.schedule(0).unwrap().unwrap().id, a);
    }

    #[test]
    fn sustained_parallel_local_preemption_keeps_cpu_ownership() {
        use std::sync::Arc;

        const CPUS: usize = 4;
        const PER_CPU: usize = 4;
        const ROUNDS: usize = 20_000;
        let scheduler = Arc::new(Scheduler::<{ CPUS * PER_CPU }, CPUS>::new());
        let mut ids = [[0u64; PER_CPU]; CPUS];
        for (cpu, cpu_ids) in ids.iter_mut().enumerate() {
            for (index, id) in cpu_ids.iter_mut().enumerate() {
                let slot = cpu * PER_CPU + index;
                *id = scheduler
                    .create(0x1000 + (slot as u64) * 0x1000, 0x400000, 0x800000)
                    .unwrap();
                scheduler.pin_ready_to_cpu(*id, cpu).unwrap();
            }
        }

        let workers: std::vec::Vec<_> = (0..CPUS)
            .map(|cpu| {
                let scheduler = Arc::clone(&scheduler);
                let cpu_ids = ids[cpu];
                std::thread::spawn(move || {
                    let frame = 0xffff_8000_0000_1000 + (cpu as u64) * 0x10;
                    let first = scheduler.schedule_local(cpu).unwrap().unwrap();
                    assert!(cpu_ids.contains(&first.id));
                    for _ in 0..ROUNDS {
                        let next = scheduler.preempt_frame_local(cpu, frame).unwrap();
                        assert!(next.is_some_and(|process| cpu_ids.contains(&process.id)));
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }

        let mut running_by_cpu = [0usize; CPUS];
        for (cpu, cpu_ids) in ids.iter().enumerate() {
            for id in cpu_ids {
                match scheduler.process(*id).unwrap().state {
                    ProcessState::Running { cpu: owner } => {
                        assert_eq!(owner, cpu);
                        running_by_cpu[owner] += 1;
                    }
                    ProcessState::Ready => {}
                    state => panic!("estado inesperado após preempção: {state:?}"),
                }
            }
        }
        assert!(running_by_cpu.iter().all(|count| *count == 1));
    }

    #[test]
    fn local_frame_preemption_keeps_process_on_its_cpu() {
        let scheduler = Scheduler::<3, 3>::new();
        let context = CpuContext {
            rip: 0x400000,
            rsp: 0x800000,
            rflags: 0x202,
            ..CpuContext::default()
        };
        let ids = [
            scheduler.create(0x1000, context.rip, context.rsp).unwrap(),
            scheduler.create(0x2000, context.rip, context.rsp).unwrap(),
            scheduler.create(0x3000, context.rip, context.rsp).unwrap(),
        ];
        for (cpu, id) in ids.into_iter().enumerate() {
            assert_eq!(scheduler.schedule(cpu).unwrap().unwrap().id, id);
        }
        let frame_addresses = [
            0xffff_8000_0000_1000,
            0xffff_8000_0000_1010,
            0xffff_8000_0000_1020,
        ];
        for (cpu, (id, frame)) in ids.into_iter().zip(frame_addresses).enumerate() {
            let next = scheduler.preempt_frame_local(cpu, frame).unwrap().unwrap();
            assert_eq!(next.id, id);
            assert!(matches!(next.state, ProcessState::Running { cpu: owner } if owner == cpu));
        }
    }

    #[test]
    fn ready_process_can_be_pinned_to_an_explicit_cpu_queue() {
        let scheduler = Scheduler::<4, 4>::new();
        let first = scheduler.create(0x1000, 0x400000, 0x800000).unwrap();
        let second = scheduler.create(0x2000, 0x500000, 0x900000).unwrap();
        let third = scheduler.create(0x3000, 0x600000, 0xa00000).unwrap();

        assert_eq!(scheduler.pin_ready_to_cpu(second, 3), Ok(()));
        assert_eq!(scheduler.schedule_local(1).unwrap(), None);
        assert_eq!(scheduler.schedule_local(3).unwrap().unwrap().id, second);
        assert_eq!(scheduler.schedule_local(0).unwrap().unwrap().id, first);
        assert_eq!(scheduler.schedule_local(2).unwrap().unwrap().id, third);
    }

    #[test]
    fn cpu_local_queues_distribute_work_and_allow_idle_cpu_stealing() {
        let q = Scheduler::<4, 3>::new();
        let id = q.create(0x1000, 0x400000, 0x800000).unwrap();
        let process = q.schedule(2).unwrap().unwrap();
        assert_eq!(process.id, id);
        assert_eq!(q.current(2).unwrap().id, id);
        assert_eq!(q.current(0), None);
        assert_eq!(q.current(1), None);
    }
    #[test]
    fn exit_removes_process_from_future_dispatch() {
        let q = Scheduler::<2, 1>::new();
        let id = q.create(0x1000, 0x400000, 0x800000).unwrap();
        q.schedule(0).unwrap();
        q.exit(0, 7).unwrap();
        assert_eq!(
            q.process(id).unwrap().state,
            ProcessState::Exited { code: 7 }
        );
        assert_eq!(q.schedule(0).unwrap(), None);
    }

    #[test]
    fn destroy_removes_ready_and_blocked_tasks_but_refuses_running_task() {
        let scheduler = Scheduler::<3, 1>::new();
        let process_blocked = scheduler.create(0x1000, 0x400000, 0x800000).unwrap();
        let process_running = scheduler.create(0x2000, 0x500000, 0x900000).unwrap();
        let process_ready = scheduler.create(0x3000, 0x600000, 0xa00000).unwrap();

        assert_eq!(scheduler.schedule(0).unwrap().unwrap().id, process_blocked);
        scheduler.block(0).unwrap();
        assert_eq!(scheduler.schedule(0).unwrap().unwrap().id, process_running);

        assert_eq!(
            scheduler.destroy(process_running, -9),
            Err(ProcessError::Busy)
        );
        assert_eq!(scheduler.destroy(process_ready, -9), Ok(()));
        assert_eq!(scheduler.destroy(process_blocked, -15), Ok(()));
        assert_eq!(
            scheduler.process(process_ready).unwrap().state,
            ProcessState::Exited { code: -9 }
        );
        assert_eq!(
            scheduler.process(process_blocked).unwrap().state,
            ProcessState::Exited { code: -15 }
        );
        assert_eq!(
            scheduler.process(process_running).unwrap().state,
            ProcessState::Running { cpu: 0 }
        );
        scheduler.exit(0, 0).unwrap();
        assert_eq!(scheduler.schedule(0).unwrap(), None);
    }

    #[test]
    fn destroy_rejects_unknown_and_already_exited_tasks() {
        let scheduler = Scheduler::<1, 1>::new();
        assert_eq!(scheduler.destroy(99, 0), Err(ProcessError::UnknownProcess));
        let id = scheduler.create(0x1000, 0x400000, 0x800000).unwrap();
        scheduler.schedule(0).unwrap();
        scheduler.exit(0, 0).unwrap();
        assert_eq!(scheduler.destroy(id, 1), Err(ProcessError::NotRunnable));
    }

    #[test]
    fn current_process_can_drop_root_and_cannot_regain_it() {
        let scheduler = Scheduler::<2, 1>::new();
        let id = scheduler.create(0x1000, 0x400000, 0x800000).unwrap();
        scheduler.schedule(0).unwrap();
        scheduler
            .change_current_credentials(0, id, CredentialChange::SupplementaryGroups(&[42]))
            .unwrap();
        scheduler
            .change_current_credentials(0, id, CredentialChange::Group(1001))
            .unwrap();
        scheduler
            .change_current_credentials(0, id, CredentialChange::User(1000))
            .unwrap();
        let security = scheduler.current(0).unwrap().security;
        assert_eq!(
            security.credentials,
            Credentials {
                uid: 1000,
                gid: 1001
            }
        );
        assert_eq!(security.groups(), &[42]);
        assert_eq!(
            scheduler.change_current_credentials(0, id, CredentialChange::User(0)),
            Err(crate::linux_abi::EPERM)
        );
        assert_eq!(
            scheduler.change_current_credentials(
                0,
                id,
                CredentialChange::SupplementaryGroups(&[0])
            ),
            Err(crate::linux_abi::EPERM)
        );
        assert_eq!(scheduler.current(0).unwrap().security, security);
    }

    #[test]
    fn clone_thread_shares_group_and_join_blocks_then_collects_exit_status() {
        let scheduler = Scheduler::<4, 3>::new();
        let pool = KernelStackPool::<4, 4096>::new();
        let security = ProcessSecurityContext::new(
            Credentials {
                uid: 1000,
                gid: 1000,
            },
            &[10, 20],
            0o027,
        )
        .unwrap();
        let parent = scheduler
            .create_with_stack_pool_and_security(
                &pool,
                0x1000,
                CpuContext {
                    rip: 0x400000,
                    rsp: 0x800000,
                    rflags: 0x202,
                    ..CpuContext::default()
                },
                security,
            )
            .unwrap();
        let thread = scheduler
            .clone_thread(
                &pool,
                parent,
                CpuContext {
                    rip: 0x401000,
                    rsp: 0x810000,
                    rflags: 0x202,
                    ..CpuContext::default()
                },
            )
            .unwrap();

        let parent_snapshot = scheduler.process(parent).unwrap();
        let thread_snapshot = scheduler.process(thread).unwrap();
        assert_eq!(thread_snapshot.page_table, parent_snapshot.page_table);
        assert_eq!(thread_snapshot.thread_group_id, parent);
        assert!(thread_snapshot.is_thread());
        assert_eq!(thread_snapshot.security, security);
        assert_ne!(
            thread_snapshot.kernel_stack_top,
            parent_snapshot.kernel_stack_top
        );

        assert_eq!(scheduler.schedule(0).unwrap().unwrap().id, parent);
        assert_eq!(scheduler.schedule(1).unwrap().unwrap().id, thread);
        assert_eq!(scheduler.join_current(0, thread), Ok(JoinStatus::Blocked));
        assert_eq!(scheduler.exit(1, 42), Ok(()));
        assert_eq!(scheduler.schedule(0).unwrap().unwrap().id, parent);
        assert_eq!(scheduler.reap(thread, &pool), Err(ProcessError::Busy));
        assert_eq!(
            scheduler.join_current(0, thread),
            Ok(JoinStatus::Exited(42))
        );

        scheduler.exit(0, 0).unwrap();
        assert_eq!(scheduler.reap(parent, &pool), Err(ProcessError::Busy));
        assert!(matches!(scheduler.reap(thread, &pool), Ok(())));
        scheduler.reap(parent, &pool).unwrap();
        assert!(pool.allocate().is_some());
    }

    #[test]
    fn clone_thread_rejects_foreign_parent_and_returns_reserved_stack() {
        let scheduler = Scheduler::<2, 1>::new();
        let pool = KernelStackPool::<1, 4096>::new();
        let error = scheduler.clone_thread(
            &pool,
            99,
            CpuContext {
                rip: 0x400000,
                rsp: 0x800000,
                rflags: 0x202,
                ..CpuContext::default()
            },
        );
        assert_eq!(error, Err(ProcessError::UnknownProcess));
        assert!(pool.allocate().is_some());
    }

    #[test]
    fn join_rejects_wait_cycles_in_a_thread_group() {
        let scheduler = Scheduler::<4, 3>::new();
        let parent = scheduler
            .create_with_context(
                0x1000,
                CpuContext {
                    rip: 0x400000,
                    rsp: 0x800000,
                    rflags: 0x202,
                    ..CpuContext::default()
                },
            )
            .unwrap();
        let pool = KernelStackPool::<2, 4096>::new();
        let first = scheduler
            .clone_thread(
                &pool,
                parent,
                CpuContext {
                    rip: 0x401000,
                    rsp: 0x810000,
                    rflags: 0x202,
                    ..CpuContext::default()
                },
            )
            .unwrap();
        let second = scheduler
            .clone_thread(
                &pool,
                parent,
                CpuContext {
                    rip: 0x402000,
                    rsp: 0x820000,
                    rflags: 0x202,
                    ..CpuContext::default()
                },
            )
            .unwrap();
        assert_eq!(scheduler.schedule(0).unwrap().unwrap().id, parent);
        assert_eq!(scheduler.schedule(1).unwrap().unwrap().id, first);
        assert_eq!(scheduler.schedule(2).unwrap().unwrap().id, second);
        assert_eq!(scheduler.join_current(1, second), Ok(JoinStatus::Blocked));
        assert_eq!(
            scheduler.join_current(2, first),
            Err(ProcessError::Deadlock)
        );
    }

    #[test]
    fn user_fault_isolated_and_resources_can_be_reaped_with_retry() {
        let scheduler = Scheduler::<1, 1>::new();
        let pool = KernelStackPool::<1, 4096>::new();
        let id = scheduler
            .create_with_stack_pool(
                &pool,
                0x1000,
                CpuContext {
                    rip: 0x400000,
                    rsp: 0x800000,
                    rflags: 0x202,
                    ..CpuContext::default()
                },
            )
            .unwrap();
        scheduler.schedule(0).unwrap();
        assert_eq!(scheduler.reapable_ids(), [None]);
        scheduler.fault(0, 14, 5, 0x7000).unwrap();
        assert_eq!(scheduler.reapable_ids(), [Some(id)]);
        assert_eq!(scheduler.schedule(0).unwrap(), None);
        assert_eq!(
            scheduler.process(id).unwrap().state,
            ProcessState::Faulted {
                vector: 14,
                error_code: 5,
                address: 0x7000,
            }
        );
        assert_eq!(
            scheduler.reap_with(id, &pool, |_| Err("temporarily busy")),
            Err(ReapError::Cleanup("temporarily busy"))
        );
        assert_eq!(
            scheduler.process(id).unwrap().state,
            ProcessState::Faulted {
                vector: 14,
                error_code: 5,
                address: 0x7000,
            }
        );
        scheduler
            .reap_with(id, &pool, |_| Ok::<(), ()>(()))
            .unwrap();
        assert_eq!(scheduler.process(id), None);
        assert_eq!(scheduler.reapable_ids(), [None]);
    }

    #[test]
    fn reaper_refuses_live_processes_on_two_cpus_then_reclaims_after_exit() {
        let scheduler = Scheduler::<2, 2>::new();
        let pool = KernelStackPool::<2, 4096>::new();
        let context = CpuContext {
            rip: 0x400000,
            rsp: 0x800000,
            rflags: 0x202,
            ..CpuContext::default()
        };
        let first = scheduler
            .create_with_stack_pool(&pool, 0x1000, context)
            .unwrap();
        let second = scheduler
            .create_with_stack_pool(&pool, 0x2000, context)
            .unwrap();

        let cpu0 = scheduler.schedule(0).unwrap().unwrap();
        let cpu1 = scheduler.schedule(1).unwrap().unwrap();
        assert_ne!(cpu0.id, cpu1.id);
        assert_eq!(
            scheduler.reap(cpu0.id, &pool),
            Err(ProcessError::NotRunnable)
        );
        assert_eq!(
            scheduler.reap(cpu1.id, &pool),
            Err(ProcessError::NotRunnable)
        );
        assert!(scheduler.process(first).is_some());
        assert!(scheduler.process(second).is_some());

        scheduler.exit(0, 0).unwrap();
        scheduler.exit(1, 0).unwrap();
        assert_eq!(scheduler.schedule(0).unwrap(), None);
        assert_eq!(scheduler.schedule(1).unwrap(), None);
        scheduler.reap(first, &pool).unwrap();
        scheduler.reap(second, &pool).unwrap();
        assert_eq!(scheduler.process(first), None);
        assert_eq!(scheduler.process(second), None);
    }

    #[test]
    fn rejects_bad_cpu_and_full_table() {
        let q = Scheduler::<1, 1>::new();
        q.create(0x1000, 0x400000, 0x800000).unwrap();
        assert_eq!(
            q.create(0x2000, 0x500000, 0x900000),
            Err(ProcessError::Full)
        );
        assert_eq!(q.schedule(1), Err(ProcessError::InvalidCpu));
    }

    #[test]
    fn blocked_process_wakes_back_into_ready_queue() {
        let q = Scheduler::<2, 1>::new();
        let id = q.create(0x1000, 0x400000, 0x800000).unwrap();
        q.schedule(0).unwrap();
        q.block(0).unwrap();
        assert_eq!(q.schedule(0).unwrap(), None);
        q.wake(id).unwrap();
        assert_eq!(q.schedule(0).unwrap().unwrap().id, id);
    }

    #[test]
    fn suspended_frame_resumes_on_its_owner_cpu() {
        let q = Scheduler::<2, 2>::new();
        let id = q.create(0x1000, 0x400000, 0x800000).unwrap();
        assert_eq!(q.schedule(0).unwrap().unwrap().id, id);
        q.block_frame(0, 0x9000).unwrap();
        assert_eq!(q.schedule(0).unwrap(), None);

        q.wake(id).unwrap();
        assert_eq!(q.pin_ready_to_cpu(id, 1), Err(ProcessError::NotRunnable));
        assert_eq!(q.schedule(1).unwrap(), None);
        let task = q.schedule(0).unwrap().unwrap();
        assert_eq!(task.id, id);
        assert_eq!(task.interrupt_frame, 0x9000);
        assert_eq!(task.resume_cpu, None);
    }

    #[test]
    fn foreground_task_stays_on_the_cpu_that_owns_its_continuation() {
        let q = Scheduler::<2, 2>::new();
        let id = q
            .insert_process_on_cpu(
                ProcessInit {
                    page_table: 0x1000,
                    context: CpuContext {
                        rip: 0x400000,
                        rsp: 0x800000,
                        rflags: 0x202,
                        ..CpuContext::default()
                    },
                    kernel_stack_top: 0,
                    interrupt_frame: 0,
                    security: ProcessSecurityContext::ROOT,
                    kernel_stack_owner: None,
                    thread_parent: None,
                },
                Some(0),
                false,
            )
            .unwrap();

        assert_eq!(q.schedule(1).unwrap(), None);
        assert_eq!(q.schedule(0).unwrap().unwrap().id, id);
        q.block(0).unwrap();
        q.wake(id).unwrap();
        assert_eq!(q.schedule(1).unwrap(), None);
        assert_eq!(q.schedule(0).unwrap().unwrap().id, id);
    }

    #[test]
    fn preemption_saves_full_context_before_round_robin_dispatch() {
        let q = Scheduler::<2, 1>::new();
        let a = q.create(0x1000, 0x400000, 0x800000).unwrap();
        let b = q.create(0x2000, 0x500000, 0x900000).unwrap();
        assert_eq!(q.schedule(0).unwrap().unwrap().id, a);
        let saved = CpuContext {
            rax: 44,
            rip: 0x401234,
            rsp: 0x800ff0,
            rflags: 0x202,
            ..CpuContext::default()
        };
        assert_eq!(q.preempt(0, saved).unwrap().unwrap().id, b);
        assert_eq!(q.schedule(0).unwrap().unwrap().context, saved);
    }

    #[test]
    fn refuses_contexts_outside_user_range_or_with_privileged_flags() {
        let q = Scheduler::<2, 1>::new();
        assert_eq!(
            q.create(0x1000, 0xffff_8000_0000_0000, 0x800000),
            Err(ProcessError::NotRunnable)
        );
        assert_eq!(
            q.create_with_context(
                0x1000,
                CpuContext {
                    rip: 0x400000,
                    rsp: 0x800000,
                    rflags: 0x3202,
                    ..CpuContext::default()
                }
            ),
            Err(ProcessError::NotRunnable)
        );
    }

    #[test]
    fn process_identity_and_groups_are_attached_to_the_scheduled_process() {
        let scheduler = Scheduler::<2, 1>::new();
        let identity = ProcessSecurityContext::new(
            Credentials {
                uid: 1000,
                gid: 100,
            },
            &[7, 9],
            0o027,
        )
        .unwrap();
        let id = scheduler
            .create_with_security_context(
                0x1000,
                CpuContext {
                    rip: 0x400000,
                    rsp: 0x800000,
                    rflags: 0x202,
                    ..CpuContext::default()
                },
                identity,
            )
            .unwrap();
        let process = scheduler.process(id).unwrap();
        let access = process.security.access_context();
        assert_eq!(
            access.credentials,
            Credentials {
                uid: 1000,
                gid: 100
            }
        );
        assert_eq!(access.supplementary_groups, &[7, 9]);
        assert_eq!(access.umask, 0o027);
        assert_eq!(
            ProcessSecurityContext::new(Credentials { uid: 1, gid: 1 }, &[0; 9], 0),
            None
        );
    }

    #[test]
    fn scales_fixed_metadata_queue_to_ten_thousand_processes() {
        std::thread::Builder::new()
            .name("process-queue-scale".into())
            .stack_size(16 * 1024 * 1024)
            .spawn(|| {
                const PROCESSES: usize = 10_000;
                static SCHEDULER: Scheduler<PROCESSES, 1> = Scheduler::new();
                let scheduler = &SCHEDULER;
                for expected_id in 1..=PROCESSES as u64 {
                    let id = scheduler.create(0x1000, 0x400000, 0x800000).unwrap();
                    assert_eq!(id, expected_id);
                }
                for expected_id in 1..=PROCESSES as u64 {
                    assert_eq!(scheduler.schedule(0).unwrap().unwrap().id, expected_id);
                }
                assert_eq!(scheduler.current(0).unwrap().id, PROCESSES as u64);
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn kernel_stack_is_exclusive_transferred_and_reclaimed_by_reaper() {
        let pool = KernelStackPool::<1, 8192>::new();
        let scheduler = Scheduler::<1, 1>::new();
        let context = CpuContext {
            rip: 0x400000,
            rsp: 0x800000,
            rflags: 0x202,
            ..CpuContext::default()
        };
        let id = scheduler
            .create_with_stack_pool(&pool, 0x1000, context)
            .unwrap();
        let process = scheduler.process(id).unwrap();
        assert_eq!(process.kernel_stack_owner, Some(0));
        assert_eq!(
            process.interrupt_frame,
            process.kernel_stack_top - USER_INTERRUPT_FRAME_BYTES
        );
        assert_eq!(process.interrupt_frame & 0xf, 0);
        assert!(pool.allocate().is_none());
        scheduler.schedule(0).unwrap();
        scheduler.exit(0, 0).unwrap();
        scheduler.reap(id, &pool).unwrap();
        assert_eq!(scheduler.process(id), None);
        assert!(pool.allocate().is_some());
    }

    #[test]
    fn process_resource_bundle_is_validated_and_frozen_at_dispatch() {
        let scheduler = Scheduler::<2, 1>::new();
        let id = scheduler.create(0x1000, 0x400000, 0x800000).unwrap();
        let invalid = ProcessResourceBundle {
            slot: 1,
            page_table: 0x1001,
            user_stack: 0x800000,
        };
        assert_eq!(
            scheduler.attach_resource_bundle(id, invalid),
            Err(ProcessError::NotRunnable)
        );
        let bundle = ProcessResourceBundle {
            slot: 1,
            page_table: 0x1000,
            user_stack: 0x800000,
        };
        scheduler.attach_resource_bundle(id, bundle).unwrap();
        assert_eq!(scheduler.process(id).unwrap().resources, Some(bundle));
        let other = scheduler.create(0x2000, 0x400000, 0x900000).unwrap();
        assert_eq!(
            scheduler.attach_resource_bundle(
                other,
                ProcessResourceBundle {
                    slot: bundle.slot,
                    page_table: 0x2000,
                    user_stack: 0x900000,
                },
            ),
            Err(ProcessError::Duplicate)
        );
        assert_eq!(
            scheduler.attach_resource_bundle(id, bundle),
            Err(ProcessError::NotRunnable)
        );
        scheduler.schedule(0).unwrap();
        assert_eq!(
            scheduler.attach_resource_bundle(id, bundle),
            Err(ProcessError::NotRunnable)
        );
    }

    #[test]
    fn dynamically_reuses_process_slots_after_exit_and_reap() {
        let pool = KernelStackPool::<1, 8192>::new();
        let scheduler = Scheduler::<1, 1>::new();
        let context = CpuContext {
            rip: 0x400000,
            rsp: 0x800000,
            rflags: 0x202,
            ..CpuContext::default()
        };
        for _ in 0..10_000 {
            let id = scheduler
                .create_with_stack_pool(&pool, 0x1000, context)
                .unwrap();
            assert_eq!(scheduler.stats().active, 1);
            scheduler.schedule(0).unwrap();
            scheduler.exit(0, 0).unwrap();
            assert_eq!(scheduler.stats().exited, 1);
            scheduler.reap(id, &pool).unwrap();
            assert_eq!(scheduler.stats().active, 0);
        }
    }

    #[test]
    fn generic_reaper_runs_cleanup_outside_scheduler_lock_and_reuses_slot() {
        let scheduler = Scheduler::<1, 1>::new();
        let pool = KernelStackPool::<1, 4096>::new();
        let id = scheduler
            .create_with_stack_pool(
                &pool,
                0x1000,
                CpuContext {
                    rip: 0x400000,
                    rsp: 0x800000,
                    rflags: 0x202,
                    ..CpuContext::default()
                },
            )
            .unwrap();
        scheduler.schedule(0).unwrap();
        scheduler.exit(0, 9).unwrap();
        scheduler
            .reap_with(id, &pool, |process| {
                assert_eq!(process.id, id);
                // This would deadlock if callback ran while scheduler state was locked.
                assert_eq!(
                    scheduler.process(id).unwrap().state,
                    ProcessState::Reaping { code: 9 }
                );
                Ok::<(), ()>(())
            })
            .unwrap();
        let replacement = scheduler
            .create_with_stack_pool(
                &pool,
                0x1000,
                CpuContext {
                    rip: 0x400000,
                    rsp: 0x800000,
                    rflags: 0x202,
                    ..CpuContext::default()
                },
            )
            .unwrap();
        assert_ne!(replacement, id);
    }

    #[test]
    fn failed_generic_cleanup_restores_exited_state_for_retry() {
        let scheduler = Scheduler::<1, 1>::new();
        let pool = KernelStackPool::<1, 4096>::new();
        let id = scheduler
            .create_with_stack_pool(
                &pool,
                0x1000,
                CpuContext {
                    rip: 0x400000,
                    rsp: 0x800000,
                    rflags: 0x202,
                    ..CpuContext::default()
                },
            )
            .unwrap();
        scheduler.schedule(0).unwrap();
        scheduler.exit(0, 3).unwrap();
        assert_eq!(
            scheduler.reap_with(id, &pool, |_| Err("busy")),
            Err(ReapError::Cleanup("busy"))
        );
        assert_eq!(
            scheduler.process(id).unwrap().state,
            ProcessState::Exited { code: 3 }
        );
        scheduler
            .reap_with(id, &pool, |_| Ok::<(), ()>(()))
            .unwrap();
    }
}

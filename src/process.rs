//! Tabela fixa de processos e fila de execução compartilhada entre CPUs.
//! Escalonador e contrato de recursos por processo. A camada x86_64 aplica o
//! frame de interrupção, CR3 e RSP0 escolhidos pelo escalonador.
use crate::sync::IrqSpinLock;
use crate::vfs::Credentials;
use core::sync::atomic::{AtomicU64, Ordering};

/// Bytes occupied by the x86_64 return frame plus all saved general registers.
pub const USER_INTERRUPT_FRAME_BYTES: u64 = 22 * 8;

/// Pilhas kernel fixas, alinhadas e exclusivas. O token retornado por `allocate`
/// é transferido ao scheduler junto com o processo; nenhum slot pode ser
/// alocado duas vezes enquanto estiver em uso.
pub struct KernelStackPool<const N: usize, const BYTES: usize> {
    state: IrqSpinLock<KernelStackPoolState<N, BYTES>>,
}

struct KernelStackPoolState<const N: usize, const BYTES: usize> {
    stacks: [KernelStackStorage<BYTES>; N],
    used: [bool; N],
}

#[repr(C, align(4096))]
struct KernelStackStorage<const BYTES: usize>([u8; BYTES]);

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
            state: IrqSpinLock::new(KernelStackPoolState {
                stacks: [const { KernelStackStorage([0; BYTES]) }; N],
                used: [false; N],
            }),
        }
    }

    pub fn allocate(&self) -> Option<KernelStack> {
        if BYTES < 4096 || BYTES % 4096 != 0 {
            return None;
        }
        let mut state = self.state.lock();
        let slot = state.used.iter().position(|used| !used)?;
        state.used[slot] = true;
        let base = stack_base::<N, BYTES>(&state, slot);
        Some(KernelStack {
            slot,
            top: base.checked_add(BYTES as u64)?,
        })
    }

    /// Liberação apenas após o processo deixar de executar nesta pilha.
    pub fn release(&self, stack: KernelStack) -> Result<(), ProcessError> {
        self.release_slot(stack.slot, stack.top)
    }

    fn release_slot(&self, slot: usize, top: u64) -> Result<(), ProcessError> {
        let mut state = self.state.lock();
        if slot >= N || !state.used[slot] {
            return Err(ProcessError::UnknownProcess);
        }
        let expected = stack_base::<N, BYTES>(&state, slot) + BYTES as u64;
        if top != expected {
            return Err(ProcessError::NotRunnable);
        }
        state.used[slot] = false;
        Ok(())
    }
}

impl<const N: usize, const BYTES: usize> Default for KernelStackPool<N, BYTES> {
    fn default() -> Self {
        Self::new()
    }
}

fn stack_base<const N: usize, const BYTES: usize>(
    state: &KernelStackPoolState<N, BYTES>,
    slot: usize,
) -> u64 {
    #[cfg(target_os = "none")]
    {
        state.stacks[slot].0.as_ptr() as u64
    }
    #[cfg(not(target_os = "none"))]
    {
        let _storage = &state.stacks[slot];
        0xffff_8000_0000_0000u64 + (slot as u64) * BYTES as u64
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
    /// Identificador do processo líder compartilhado pelas threads do mesmo grupo.
    pub thread_group_id: u64,
    /// Alvo aguardado pela thread, até ela consumir o resultado do join.
    pub join_target: Option<u64>,
    join_claimed: bool,
    joinable: bool,
    pub page_table: u64,
    pub entry: u64,
    pub user_stack: u64,
    pub context: CpuContext,
    /// Topo da pilha privilegiada exclusiva; 16-byte aligned, endereço virtual.
    pub kernel_stack_top: u64,
    /// Índice da pilha possuída no KernelStackPool; None apenas no modelo host.
    pub kernel_stack_owner: Option<usize>,
    /// Frame de retorno salvo em interrupção (ponteiro virtual no espaço kernel).
    pub interrupt_frame: u64,
    pub state: ProcessState,
    /// Identidade POSIX immutable para autorizar VFS/syscalls por processo.
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
    pub credentials: Credentials,
    pub supplementary_groups: [u32; 8],
    pub group_count: u8,
    pub umask: u16,
}

impl ProcessSecurityContext {
    pub const ROOT: Self = Self {
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
            credentials,
            supplementary_groups,
            group_count: groups.len() as u8,
            umask: umask & 0o777,
        })
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

impl Process {
    pub const fn is_thread(&self) -> bool {
        self.id != self.thread_group_id
    }

    const EMPTY: Self = Self {
        id: 0,
        thread_group_id: 0,
        join_target: None,
        join_claimed: false,
        joinable: false,
        page_table: 0,
        entry: 0,
        user_stack: 0,
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
pub struct Scheduler<const P: usize, const C: usize> {
    state: IrqSpinLock<State<P, C>>,
}

impl<const P: usize, const C: usize> Scheduler<P, C> {
    pub const fn new() -> Self {
        Self {
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
        let stack = pool.allocate().ok_or(ProcessError::Full)?;
        let Some(frame) = stack.top.checked_sub(USER_INTERRUPT_FRAME_BYTES) else {
            let _ = pool.release(stack);
            return Err(ProcessError::NotRunnable);
        };
        match self.create_with_owned_execution_resources(
            page_table,
            context,
            stack.top,
            frame,
            security,
            Some(stack.slot),
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
            thread_group_id: if thread_group_id == 0 {
                id
            } else {
                thread_group_id
            },
            join_target: None,
            join_claimed: false,
            joinable,
            page_table,
            entry: context.rip,
            user_stack: context.rsp,
            context,
            kernel_stack_top,
            kernel_stack_owner,
            interrupt_frame,
            state: ProcessState::Ready,
            security,
            resources: None,
        };
        let cpu = least_loaded_queue(&mut s).ok_or(ProcessError::InvalidCpu)?;
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
                s.processes[slot].interrupt_frame = frame_address;
            }
        }
        self.schedule(cpu)
    }

    /// Reagenda o processo interrompido na fila local, sem roubo entre CPUs.
    /// Útil para a prova SMP: mantém cada frame/stack na CPU que o produziu.
    pub fn preempt_frame_local(
        &self,
        cpu: usize,
        frame_address: u64,
    ) -> Result<Option<Process>, ProcessError> {
        if !is_kernel_address(frame_address) || frame_address & 0xf != 0 {
            return Err(ProcessError::NotRunnable);
        }
        {
            let mut state = self.state.lock();
            if cpu >= C {
                return Err(ProcessError::InvalidCpu);
            }
            let slot = state.current_slot[cpu];
            if slot == P || slot >= P {
                return Err(ProcessError::NotRunnable);
            }
            if !matches!(state.processes[slot].state, ProcessState::Running { cpu: owner } if owner == cpu)
            {
                return Err(ProcessError::NotRunnable);
            }
            state.processes[slot].interrupt_frame = frame_address;
            state.processes[slot].state = ProcessState::Ready;
            push_slot(&mut state, slot, cpu)?;
            state.current_slot[cpu] = P;
            state.prefer_steal[cpu] = false;
        }
        self.schedule_local(cpu)
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
        let mut s = self.state.lock();
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
                push_slot(&mut s, slot, cpu)?;
                s.prefer_steal[cpu] = true;
            }
            s.current_slot[cpu] = P;
        }
        while let Some(queue) = if allow_steal {
            next_ready_queue(&s, cpu, s.prefer_steal[cpu])
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
            s.current_slot[cpu] = queued;
            s.prefer_steal[cpu] = false;
            return Ok(Some(s.processes[queued]));
        }
        Ok(None)
    }

    pub fn block(&self, cpu: usize) -> Result<(), ProcessError> {
        self.stop_current(cpu, ProcessState::Blocked)
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
        let cpu = least_loaded_queue(&mut s).ok_or(ProcessError::InvalidCpu)?;
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

    /// Associa ao processo o token do bundle mantido pelo backend de memória.
    /// A associação é permitida somente enquanto a tarefa ainda está pronta,
    /// antes de qualquer CPU poder despachá-la.
    pub fn attach_resource_bundle(
        &self,
        id: u64,
        bundle: ProcessResourceBundle,
    ) -> Result<(), ProcessError> {
        if bundle.page_table == 0
            || bundle.page_table & 0xfff != 0
            || bundle.user_stack == 0
            || bundle.slot >= 3
        {
            return Err(ProcessError::NotRunnable);
        }
        let mut state = self.state.lock();
        let slot = find(&state.processes, id).ok_or(ProcessError::UnknownProcess)?;
        let process = &mut state.processes[slot];
        if process.state != ProcessState::Ready
            || process.resources.is_some()
            || process.page_table != bundle.page_table
            || process.user_stack != bundle.user_stack
        {
            return Err(ProcessError::NotRunnable);
        }
        process.resources = Some(bundle);
        Ok(())
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
impl<const P: usize, const C: usize> Default for Scheduler<P, C> {
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
        let cpu = least_loaded_queue(state).ok_or(ProcessError::InvalidCpu)?;
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
    if prefer_steal {
        (1..C)
            .map(|offset| (cpu + offset) % C)
            .find(|&queue| s.ready_len[queue] != 0)
            .or_else(|| (s.ready_len[cpu] != 0).then_some(cpu))
    } else {
        (s.ready_len[cpu] != 0).then_some(cpu).or_else(|| {
            (1..C)
                .map(|offset| (cpu + offset) % C)
                .find(|&q| s.ready_len[q] != 0)
        })
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
        scheduler.fault(0, 14, 5, 0x7000).unwrap();
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
            slot: 3,
            page_table: 0x1000,
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

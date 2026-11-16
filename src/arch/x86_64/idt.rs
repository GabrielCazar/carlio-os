//! IDT mínima para capturar faults e exceções de CPU em polling serial.

use core::arch::{asm, global_asm};
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

global_asm!(
    r#"
    .text
    .macro ISR_NOERR n
    .global exception_stub_\n
exception_stub_\n:
        pushq $0
        pushq $\n
        jmp exception_common
    .endm

    .macro ISR_ERR n
    .global exception_stub_\n
exception_stub_\n:
        pushq $\n
        jmp exception_common
    .endm

    ISR_NOERR 0
    ISR_NOERR 1
    ISR_NOERR 2
    ISR_NOERR 3
    ISR_NOERR 4
    ISR_NOERR 5
    ISR_NOERR 6
    ISR_NOERR 7
    ISR_ERR   8
    ISR_NOERR 9
    ISR_ERR   10
    ISR_ERR   11
    ISR_ERR   12
    ISR_ERR   13
    ISR_ERR   14
    ISR_NOERR 15
    ISR_NOERR 16
    ISR_ERR   17
    ISR_NOERR 18
    ISR_NOERR 19
    ISR_NOERR 20
    ISR_ERR   21
    ISR_NOERR 22
    ISR_NOERR 23
    ISR_NOERR 24
    ISR_NOERR 25
    ISR_NOERR 26
    ISR_NOERR 27
    ISR_NOERR 28
    ISR_ERR   29
    ISR_ERR   30
    ISR_NOERR 31

    .global watchdog_timer_stub
watchdog_timer_stub:
        pushq $0
        pushq $32
        jmp exception_common

    .global scheduler_timer_stub
scheduler_timer_stub:
        pushq $0
        pushq $64
        jmp exception_common

    .global scheduler_idle_resume_stub
scheduler_idle_resume_stub:
        cld
        call carlio_ap_idle_resume
        ud2

    .global tlb_shootdown_stub
tlb_shootdown_stub:
        cld
        pushq %rax
        pushq %rcx
        pushq %rdx
        pushq %rbx
        pushq %rbp
        pushq %rsi
        pushq %rdi
        pushq %r8
        pushq %r9
        pushq %r10
        pushq %r11
        pushq %r12
        pushq %r13
        pushq %r14
        pushq %r15
        movq %rsp, %r12
        andq $-16, %rsp
        call handle_tlb_shootdown_ipi
        movq %r12, %rsp
        popq %r15
        popq %r14
        popq %r13
        popq %r12
        popq %r11
        popq %r10
        popq %r9
        popq %r8
        popq %rdi
        popq %rsi
        popq %rbp
        popq %rbx
        popq %rdx
        popq %rcx
        popq %rax
        iretq

    .global carlio_ring3_test_int80_stub
carlio_ring3_test_int80_stub:
        pushq $0
        pushq $128
        jmp exception_common

    .global carlio_syscall_entry
carlio_syscall_entry:
        swapgs
        movq %rsp, %gs:8
        movq %gs:0, %rsp
        pushq %rax
        pushq %rdi
        pushq %rsi
        pushq %rdx
        pushq %r10
        pushq %r8
        pushq %r9
        pushq %rcx
        pushq %r11
        movq %gs:8, %rax
        pushq %rax
        movq %rsp, %rdi
        call carlio_syscall_dispatch
        movq %rax, 72(%rsp)
        movq 24(%rsp), %r9
        movq 32(%rsp), %r8
        movq 40(%rsp), %r10
        movq 48(%rsp), %rdx
        movq 56(%rsp), %rsi
        movq 64(%rsp), %rdi
        movq 16(%rsp), %rcx
        movq 8(%rsp), %r11
        movq 0(%rsp), %rax
        movq %rax, %gs:8
        movq 72(%rsp), %rax
        movq %gs:8, %rsp
        swapgs
        sysretq

    .global carlio_ring3_test_resume
carlio_ring3_test_resume:
        movq CARLIO_RING3_SAVED_RSP(%rip), %rsp
        popq %r15
        popq %r14
        popq %r13
        popq %r12
        popq %rbp
        popq %rbx
        retq

    .global carlio_resume_ring3_after_syscall
carlio_resume_ring3_after_syscall:
        movq %rdi, %rax
        movq CARLIO_RING3_SAVED_RSP(%rip), %rsp
        popq %r15
        popq %r14
        popq %r13
        popq %r12
        popq %rbp
        popq %rbx
        retq

    .global carlio_enter_ring3_test
carlio_enter_ring3_test:
        pushq %rbx
        pushq %rbp
        pushq %r12
        pushq %r13
        pushq %r14
        pushq %r15
        movq %rsp, CARLIO_RING3_SAVED_RSP(%rip)
        pushq $0x1b
        pushq %rsi
        pushq $0x02
        pushq $0x23
        pushq %rdi
        iretq

    .global exception_common
exception_common:
        cli
        cld
        pushq %r15
        pushq %r14
        pushq %r13
        pushq %r12
        pushq %r11
        pushq %r10
        pushq %r9
        pushq %r8
        pushq %rdi
        pushq %rsi
        pushq %rbp
        pushq %rbx
        pushq %rdx
        pushq %rcx
        pushq %rax
        movq %rsp, %rdi
        movq %rsp, %r12
        andq $-16, %rsp
        call exception_dispatch
        movq %rax, %rsp
        popq %rax
        popq %rcx
        popq %rdx
        popq %rbx
        popq %rbp
        popq %rsi
        popq %rdi
        popq %r8
        popq %r9
        popq %r10
        popq %r11
        popq %r12
        popq %r13
        popq %r14
        popq %r15
        addq $16, %rsp
        iretq
    1:
        cli
        hlt
        jmp 1b

    .section .rodata
    .align 8
    .global exception_stub_table
exception_stub_table:
    .quad exception_stub_0, exception_stub_1, exception_stub_2, exception_stub_3
    .quad exception_stub_4, exception_stub_5, exception_stub_6, exception_stub_7
    .quad exception_stub_8, exception_stub_9, exception_stub_10, exception_stub_11
    .quad exception_stub_12, exception_stub_13, exception_stub_14, exception_stub_15
    .quad exception_stub_16, exception_stub_17, exception_stub_18, exception_stub_19
    .quad exception_stub_20, exception_stub_21, exception_stub_22, exception_stub_23
    .quad exception_stub_24, exception_stub_25, exception_stub_26, exception_stub_27
    .quad exception_stub_28, exception_stub_29, exception_stub_30, exception_stub_31
    "#,
    options(att_syntax)
);

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    attributes: u8,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    const MISSING: Self = Self {
        offset_low: 0,
        selector: 0,
        ist: 0,
        attributes: 0,
        offset_mid: 0,
        offset_high: 0,
        reserved: 0,
    };

    fn interrupt_gate(address: u64, selector: u16, ist: u8) -> Self {
        Self {
            offset_low: address as u16,
            selector,
            ist: ist & 0x07,
            attributes: 0x8e,
            offset_mid: (address >> 16) as u16,
            offset_high: (address >> 32) as u32,
            reserved: 0,
        }
    }

    fn user_interrupt_gate(address: u64, selector: u16) -> Self {
        let mut gate = Self::interrupt_gate(address, selector, 0);
        gate.attributes = 0xee; // presente, DPL 3, interrupt gate de 64 bits
        gate
    }
}

#[repr(C, packed)]
struct IdtPointer {
    limit: u16,
    base: u64,
}

#[repr(C, packed)]
struct TaskStateSegment {
    reserved0: u32,
    rsp: [u64; 3],
    reserved1: u64,
    ist: [u64; 7],
    reserved2: u64,
    reserved3: u16,
    iomap_base: u16,
}

impl TaskStateSegment {
    const EMPTY: Self = Self {
        reserved0: 0,
        rsp: [0; 3],
        reserved1: 0,
        ist: [0; 7],
        reserved2: 0,
        reserved3: 0,
        iomap_base: core::mem::size_of::<Self>() as u16,
    };
}

#[repr(C)]
struct ExceptionFrame {
    rax: u64,
    rcx: u64,
    rdx: u64,
    rbx: u64,
    rbp: u64,
    rsi: u64,
    rdi: u64,
    r8: u64,
    r9: u64,
    r10: u64,
    r11: u64,
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
    vector: u64,
    error_code: u64,
    rip: u64,
    cs: u64,
    rflags: u64,
}

#[repr(C)]
pub struct UserInterruptFrame {
    pub rax: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rbx: u64,
    pub rbp: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub vector: u64,
    pub error_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub user_rsp: u64,
    pub user_ss: u64,
}

#[repr(C)]
struct LinuxSyscallFrame {
    user_rsp: u64,
    rflags: u64,
    user_rip: u64,
    r9: u64,
    r8: u64,
    r10: u64,
    rdx: u64,
    rsi: u64,
    rdi: u64,
    number: u64,
}

#[repr(C, align(16))]
struct SyscallCpuLocal {
    kernel_rsp: u64,
    user_rsp: u64,
}

static mut IDT: [IdtEntry; 256] = [IdtEntry::MISSING; 256];
static mut GDT: [u64; 7] = [0; 7];
static mut TSS: TaskStateSegment = TaskStateSegment::EMPTY;
const MAX_CPU_TABLES: usize = 64;
static mut CPU_TSS: [TaskStateSegment; MAX_CPU_TABLES] =
    [const { TaskStateSegment::EMPTY }; MAX_CPU_TABLES];
static mut CPU_GDT: [[u64; 7]; MAX_CPU_TABLES] = [[0; 7]; MAX_CPU_TABLES];
static mut SYSCALL_LOCALS: [SyscallCpuLocal; MAX_CPU_TABLES] = [const {
    SyscallCpuLocal {
        kernel_rsp: 0,
        user_rsp: 0,
    }
}; MAX_CPU_TABLES];
#[repr(align(16))]
struct DoubleFaultStack([u8; 16 * 1024]);
static mut DOUBLE_FAULT_STACK: DoubleFaultStack = DoubleFaultStack([0; 16 * 1024]);
#[repr(align(16))]
struct Ring0EntryStack([u8; 16 * 1024]);
static mut RING0_ENTRY_STACK: Ring0EntryStack = Ring0EntryStack([0; 16 * 1024]);
#[repr(align(16))]
struct CpuEntryStack([u8; 16 * 1024]);
static mut CPU_ENTRY_STACKS: [CpuEntryStack; MAX_CPU_TABLES] =
    [const { CpuEntryStack([0; 16 * 1024]) }; MAX_CPU_TABLES];
#[repr(align(16))]
struct CpuDoubleFaultStack([u8; 16 * 1024]);
static mut CPU_DOUBLE_FAULT_STACKS: [CpuDoubleFaultStack; MAX_CPU_TABLES] =
    [const { CpuDoubleFaultStack([0; 16 * 1024]) }; MAX_CPU_TABLES];

#[no_mangle]
static mut CARLIO_RING3_SAVED_RSP: u64 = 0;

pub const KERNEL_CODE_SELECTOR: u16 = 0x08;
pub const KERNEL_DATA_SELECTOR: u16 = 0x10;
pub const USER_DATA_SELECTOR: u16 = 0x1b;
pub const USER_CODE_SELECTOR: u16 = 0x23;
pub const TSS_SELECTOR: u16 = 0x28;

static TICKS: AtomicU64 = AtomicU64::new(0);
static RING3_PROBE_ACTIVE: AtomicBool = AtomicBool::new(false);
static HEARTBEAT: AtomicU64 = AtomicU64::new(0);
static LAST_HEARTBEAT: AtomicU64 = AtomicU64::new(0);
static LAST_PROGRESS_TICK: AtomicU64 = AtomicU64::new(0);
static TIMER_FRAME_SWITCH: AtomicUsize = AtomicUsize::new(0);
const WATCHDOG_TICKS: u64 = 500; // 5 s em PIT 100 Hz

unsafe extern "C" {
    static exception_stub_table: [usize; 32];
    static watchdog_timer_stub: u8;
    static scheduler_timer_stub: u8;
    static scheduler_idle_resume_stub: u8;
    static tlb_shootdown_stub: u8;
    static carlio_ring3_test_int80_stub: u8;
    static carlio_ring3_test_resume: u8;
    static carlio_syscall_entry: u8;
    fn carlio_enter_ring3_test(entry: u64, stack_top: u64) -> u64;
    fn carlio_resume_ring3_after_syscall(result: u64) -> !;
}

pub unsafe fn install() {
    unsafe { asm!("cli", options(nomem, nostack)) };
    prepare_tss_and_gdt();
    let _ = unsafe { load_shared_kernel_gdt(0) };
    if !unsafe { enable_linux_syscall_entry(0) } {
        super::write("ABI Linux: SYSCALL indisponível neste processador.\r\n");
    }
    let selector: u16 = 0x08;
    unsafe {
        let timer_stub = core::ptr::addr_of!(watchdog_timer_stub) as u64;
        IDT[32] = IdtEntry::interrupt_gate(timer_stub, selector, 0);
        let scheduler_timer = core::ptr::addr_of!(scheduler_timer_stub) as u64;
        IDT[crate::lapic::SCHEDULER_TIMER_VECTOR as usize] =
            IdtEntry::interrupt_gate(scheduler_timer, selector, 0);
        let tlb_stub = core::ptr::addr_of!(tlb_shootdown_stub) as u64;
        IDT[crate::lapic::TLB_IPI_VECTOR as usize] =
            IdtEntry::interrupt_gate(tlb_stub, selector, 0);
        let ring3_test_stub = core::ptr::addr_of!(carlio_ring3_test_int80_stub) as u64;
        IDT[0x80] = IdtEntry::user_interrupt_gate(ring3_test_stub, selector);
    }
    let stubs = core::ptr::addr_of!(exception_stub_table).cast::<usize>();
    for vector in 0..32 {
        let address = unsafe { stubs.add(vector).read() } as u64;
        unsafe {
            let ist = if vector == 8 { 1 } else { 0 };
            IDT[vector] = IdtEntry::interrupt_gate(address, selector, ist);
        }
    }
    let pointer = IdtPointer {
        limit: (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16,
        base: core::ptr::addr_of!(IDT) as u64,
    };
    unsafe {
        asm!("lidt [{}]", in(reg) &pointer, options(readonly, nostack, preserves_flags));
    }
}

/// Carrega nos APs a IDT compartilhada preparada pelo BSP.
pub unsafe fn load_shared_idt() {
    let pointer = IdtPointer {
        limit: (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16,
        base: core::ptr::addr_of!(IDT) as u64,
    };
    unsafe { asm!("lidt [{}]", in(reg) &pointer, options(readonly, nostack, preserves_flags)) };
}

/// Instala os seletores de código/dados do kernel nos APs sem reutilizar o TSS
/// do BSP (cada CPU deverá ter seu próprio TSS antes de entrar em CPL3).
pub unsafe fn load_shared_kernel_gdt(cpu: usize) -> bool {
    if cpu >= MAX_CPU_TABLES {
        return false;
    }
    let stack_top = unsafe {
        core::ptr::addr_of_mut!(CPU_ENTRY_STACKS[cpu].0)
            .cast::<u8>()
            .add(16 * 1024) as u64
    };
    let double_fault_top = unsafe {
        core::ptr::addr_of_mut!(CPU_DOUBLE_FAULT_STACKS[cpu].0)
            .cast::<u8>()
            .add(16 * 1024) as u64
    };
    let tss = core::ptr::addr_of_mut!(CPU_TSS[cpu]);
    unsafe {
        core::ptr::addr_of_mut!((*tss).rsp[0]).write_unaligned(stack_top);
        core::ptr::addr_of_mut!((*tss).ist[0]).write_unaligned(double_fault_top);
        for index in 0..5 {
            CPU_GDT[cpu][index] = core::ptr::addr_of!(GDT)
                .cast::<u64>()
                .add(index)
                .read_volatile();
        }
        let tss_base = tss as u64;
        let tss_limit = (core::mem::size_of::<TaskStateSegment>() - 1) as u64;
        CPU_GDT[cpu][5] = (tss_limit & 0xffff)
            | ((tss_base & 0x00ff_ffff) << 16)
            | (0x89 << 40)
            | (((tss_limit >> 16) & 0x0f) << 48)
            | (((tss_base >> 24) & 0xff) << 56);
        CPU_GDT[cpu][6] = tss_base >> 32;
    }
    let pointer = IdtPointer {
        limit: (core::mem::size_of::<[u64; 7]>() - 1) as u16,
        base: core::ptr::addr_of!(CPU_GDT[cpu]) as u64,
    };
    unsafe {
        asm!(
            "lgdt [{descriptor}]",
            "push 0x08",
            "lea rax, [rip + 2f]",
            "push rax",
            "retfq",
            "2:",
            "mov ax, 0x10",
            "mov ds, ax",
            "mov es, ax",
            "mov ss, ax",
            "mov ax, 0x28",
            "ltr ax",
            descriptor = in(reg) &pointer,
            out("rax") _,
        );
    }
    unsafe { enable_linux_syscall_entry(cpu) }
}

/// Configura MSRs da entrada Linux x86_64 em cada CPU. `KERNEL_GS_BASE`
/// aponta para o par de stacks da CPU; `SYSCALL` em si não troca RSP.
unsafe fn enable_linux_syscall_entry(cpu: usize) -> bool {
    if cpu >= MAX_CPU_TABLES {
        return false;
    }
    let initial_stack = unsafe {
        core::ptr::addr_of_mut!(CPU_ENTRY_STACKS[cpu].0)
            .cast::<u8>()
            .add(16 * 1024) as u64
    };
    unsafe {
        core::ptr::addr_of_mut!(SYSCALL_LOCALS[cpu].kernel_rsp).write_volatile(initial_stack)
    };
    let local = core::ptr::addr_of_mut!(SYSCALL_LOCALS[cpu]) as u64;
    let lstar = core::ptr::addr_of!(carlio_syscall_entry) as u64;
    unsafe {
        let efer = rdmsr(0xc000_0080) | 1;
        wrmsr(0xc000_0080, efer); // IA32_EFER.SCE
        wrmsr(0xc000_0081, (0x13u64 << 48) | (0x08u64 << 32)); // STAR
        wrmsr(0xc000_0082, lstar); // LSTAR
        wrmsr(0xc000_0084, (1 << 9) | (1 << 10) | (1 << 8) | (1 << 18)); // FMASK
        wrmsr(0xc000_0101, 0); // user GS base
        wrmsr(0xc000_0102, local); // kernel GS base, swapped on entry
    }
    true
}

unsafe fn rdmsr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        asm!("rdmsr", in("ecx") msr, out("eax") low, out("edx") high, options(nomem, nostack))
    };
    ((high as u64) << 32) | low as u64
}

unsafe fn wrmsr(msr: u32, value: u64) {
    unsafe {
        asm!("wrmsr", in("ecx") msr, in("eax") value as u32, in("edx") (value >> 32) as u32, options(nomem, nostack))
    };
}

/// Verifica os descritores e o seletor de tarefa instalados. Não muda CPL.
pub fn ring3_descriptors_ready() -> bool {
    let task_register: u16;
    unsafe { asm!("str {0:x}", out(reg) task_register, options(nomem, nostack, preserves_flags)) };
    let user_code = unsafe {
        core::ptr::addr_of!(GDT)
            .cast::<u64>()
            .add(4)
            .read_volatile()
    };
    let user_data = unsafe {
        core::ptr::addr_of!(GDT)
            .cast::<u64>()
            .add(3)
            .read_volatile()
    };
    let rsp0 = unsafe { core::ptr::addr_of!(TSS.rsp).cast::<u64>().read_unaligned() };
    let user_gate_attributes = unsafe {
        core::ptr::addr_of!(IDT)
            .cast::<u8>()
            .add(0x80 * core::mem::size_of::<IdtEntry>() + 5)
            .read_volatile()
    };
    KERNEL_CODE_SELECTOR == 1 << 3
        && KERNEL_DATA_SELECTOR == 2 << 3
        && USER_DATA_SELECTOR == (3 << 3) | 3
        && USER_CODE_SELECTOR == (4 << 3) | 3
        && task_register == TSS_SELECTOR
        && (user_code >> 45) & 3 == 3
        && (user_data >> 45) & 3 == 3
        && user_gate_attributes == 0xee
        && rsp0 != 0
}

/// Executa um trampoline de prova CPL3 que usa `int 0x80` para retornar ao
/// kernel. Só deve ser chamado com código/stack user mapeados e IRQs suspensas.
pub unsafe fn run_ring3_probe(entry: u64, stack_top: u64) -> u64 {
    if RING3_PROBE_ACTIVE.swap(true, Ordering::AcqRel) {
        return u64::MAX;
    }
    let result = unsafe { carlio_enter_ring3_test(entry, stack_top) };
    RING3_PROBE_ACTIVE.store(false, Ordering::Release);
    result
}

/// Instala o CR3 e o RSP0 para o próximo processo. O caller precisa estar com
/// IRQs locais desabilitadas e garantir que o kernel, IDT, TSS e a pilha atual
/// estejam mapeados identicamente nos dois espaços. O frame de retorno deve
/// ser trocado pelo stub de interrupção antes de reabilitar IRQs.
pub unsafe fn activate_process_address_space(root: u64, kernel_stack_top: u64) -> bool {
    if root == 0
        || root & 0xfff != 0
        || root & !0x000f_ffff_ffff_f000 != 0
        || kernel_stack_top & 0xf != 0
        || kernel_stack_top < 0xffff_8000_0000_0000
    {
        return false;
    }
    unsafe {
        core::ptr::addr_of_mut!(TSS.rsp[0]).write_unaligned(kernel_stack_top);
        core::ptr::addr_of_mut!(SYSCALL_LOCALS[0].kernel_rsp).write_volatile(kernel_stack_top);
        asm!("mov cr3, {}", in(reg) root, options(nostack, preserves_flags));
    }
    true
}

pub unsafe fn activate_cpu_process_address_space(
    cpu: usize,
    root: u64,
    kernel_stack_top: u64,
) -> bool {
    if cpu >= MAX_CPU_TABLES
        || root == 0
        || root & 0xfff != 0
        || root & !0x000f_ffff_ffff_f000 != 0
        || kernel_stack_top & 0xf != 0
        || kernel_stack_top < 0xffff_8000_0000_0000
    {
        return false;
    }
    unsafe {
        core::ptr::addr_of_mut!(CPU_TSS[cpu].rsp[0]).write_unaligned(kernel_stack_top);
        core::ptr::addr_of_mut!(SYSCALL_LOCALS[cpu].kernel_rsp).write_volatile(kernel_stack_top);
        asm!("mov cr3, {}", in(reg) root, options(nostack, preserves_flags));
    }
    true
}

/// Entra em CPL3 a partir de um frame de interrupção previamente preparado.
/// Usado por APs ao receberem seu primeiro processo.
pub unsafe fn enter_user_frame(frame: *mut UserInterruptFrame) -> ! {
    unsafe {
        asm!(
            "mov rsp, rdi",
            "pop rax", "pop rcx", "pop rdx", "pop rbx", "pop rbp",
            "pop rsi", "pop rdi", "pop r8", "pop r9", "pop r10",
            "pop r11", "pop r12", "pop r13", "pop r14", "pop r15",
            "add rsp, 16", "iretq",
            in("rdi") frame,
            options(noreturn)
        );
    }
}

/// Restores the bootstrap privilege stack after a process probe returned to
/// the kernel's saved caller context.
pub unsafe fn restore_bootstrap_kernel_stack() {
    let top = unsafe {
        core::ptr::addr_of_mut!(RING0_ENTRY_STACK.0)
            .cast::<u8>()
            .add(16 * 1024) as u64
    };
    unsafe { core::ptr::addr_of_mut!(TSS.rsp[0]).write_unaligned(top) };
}

/// Instala o callback executado a cada IRQ0 que interrompe código user. Ele
/// deve salvar o frame atual e devolver o frame user do próximo processo.
/// O callback retorna ponteiro válido de 16-byte aligned e configura CR3/RSP0
/// antes de devolver; IRQs permanecem desabilitadas durante a operação.
pub unsafe fn install_timer_frame_switch(
    hook: extern "C" fn(usize, *mut UserInterruptFrame) -> *mut UserInterruptFrame,
) {
    TIMER_FRAME_SWITCH.store(hook as usize, Ordering::Release);
}

/// Monta frame inicial para que o caminho comum restaure registradores e faça
/// `iretq` diretamente para CPL3. A pilha precisa pertencer ao processo e estar
/// mapeada como supervisor em todos os CR3s de kernel.
pub unsafe fn initialize_user_frame(
    stack_top: u64,
    context: carlio_os::process::CpuContext,
) -> Option<*mut UserInterruptFrame> {
    let frame_address = stack_top.checked_sub(core::mem::size_of::<UserInterruptFrame>() as u64)?;
    if stack_top & 0xf != 0 || frame_address < 0xffff_8000_0000_0000 {
        return None;
    }
    let frame = UserInterruptFrame {
        rax: context.rax,
        rcx: context.rcx,
        rdx: context.rdx,
        rbx: context.rbx,
        rbp: context.rbp,
        rsi: context.rsi,
        rdi: context.rdi,
        r8: context.r8,
        r9: context.r9,
        r10: context.r10,
        r11: context.r11,
        r12: context.r12,
        r13: context.r13,
        r14: context.r14,
        r15: context.r15,
        vector: 0,
        error_code: 0,
        rip: context.rip,
        cs: USER_CODE_SELECTOR as u64,
        rflags: context.rflags,
        user_rsp: context.rsp,
        user_ss: USER_DATA_SELECTOR as u64,
    };
    let pointer = frame_address as *mut UserInterruptFrame;
    unsafe { pointer.write(frame) };
    Some(pointer)
}

#[no_mangle]
extern "C" fn exception_dispatch(frame: *mut ExceptionFrame) -> *mut ExceptionFrame {
    let frame = unsafe { &mut *frame };
    let cpu = crate::lapic::local_id()
        .and_then(crate::cpu_index_for_lapic)
        .unwrap_or(0);
    if frame.vector == 128
        && frame.cs & 3 == 3
        && frame.rax == carlio_os::syscall::SYS_EXIT
        && crate::process_probe_is_active()
        && crate::scheduler_has_current_process(cpu)
    {
        if let Some(next) = crate::process_probe_exit(frame.rdi) {
            return next.cast();
        }
        if crate::is_secondary_cpu(cpu) {
            prepare_ap_idle_resume(frame);
            return frame;
        }
    }
    if frame.vector == 128 && frame.cs & 3 == 3 && crate::scheduler_has_current_process(cpu) {
        if let Some(outcome) = crate::dispatch_linux_syscall(
            frame.rax,
            [
                frame.rdi, frame.rsi, frame.rdx, frame.r10, frame.r8, frame.r9,
            ],
            cpu,
        ) {
            match outcome {
                carlio_os::linux_abi::SyscallOutcome::Return(value) => {
                    frame.rax = value;
                    return frame;
                }
                carlio_os::linux_abi::SyscallOutcome::Exit { status } => {
                    if let Some(next) = crate::process_probe_exit(status as u64) {
                        return next.cast();
                    }
                    if crate::is_secondary_cpu(cpu) {
                        prepare_ap_idle_resume(frame);
                    } else if crate::process_probe_is_active() {
                        let result = crate::process_probe_result();
                        resume_ring3_probe(frame, result);
                    }
                    return frame;
                }
            }
        }
    }
    if frame.vector == 128 && frame.cs & 3 == 3 && RING3_PROBE_ACTIVE.load(Ordering::Acquire) {
        if frame.rax == carlio_os::syscall::SYS_EXIT && crate::process_probe_is_active() {
            super::write("Syscall: tarefa solicitou encerramento.\r\n");
            if let Some(next) = crate::process_probe_exit(frame.rdi) {
                return next.cast();
            }
            let result = crate::process_probe_result();
            resume_ring3_probe(frame, result);
            return frame;
        }
        if frame.rax == carlio_os::syscall::SYS_KERNEL_ROUTINE_PROBE {
            let result = crate::ring3_kernel_routine_selftest();
            if result == 0x7ff && !crate::process_probe_is_done() {
                if let Some(first) = crate::begin_process_switch_probe() {
                    return first.cast();
                }
            }
            resume_ring3_probe(frame, result);
            return frame;
        }
    }
    // Qualquer exceção síncrona em CPL3 pertencente a uma tarefa do scheduler
    // encerra apenas essa tarefa; o handler de prova não é política do fault.
    if frame.cs & 3 == 3
        && frame.vector < 32
        && (crate::process_probe_is_active() || crate::scheduler_has_current_process(cpu))
    {
        let cr2 = if frame.vector == 14 {
            let address: u64;
            unsafe {
                asm!("mov {}, cr2", out(reg) address, options(nomem, nostack, preserves_flags))
            };
            address
        } else {
            0
        };
        if let Some(next) = crate::process_runtime_fault(frame.vector, frame.error_code, cr2) {
            return next.cast();
        }
        if crate::is_secondary_cpu(cpu) {
            prepare_ap_idle_resume(frame);
            return frame;
        }
        let result = crate::process_probe_result();
        resume_ring3_probe(frame, result);
        return frame;
    }
    if RING3_PROBE_ACTIVE.load(Ordering::Acquire)
        && frame.cs & 3 == 3
        && (frame.vector == 128 || frame.vector < 32)
    {
        let result = if frame.vector == 128 {
            if frame.rax == carlio_os::syscall::SYS_KERNEL_ROUTINE_PROBE {
                crate::ring3_kernel_routine_selftest()
            } else {
                carlio_os::syscall::dispatch(
                    frame.rax,
                    [
                        frame.rdi, frame.rsi, frame.rdx, frame.r10, frame.r8, frame.r9,
                    ],
                    carlio_os::syscall::SyscallContext {
                        process_id: 1,
                        uptime_ticks: TICKS.load(Ordering::Relaxed),
                    },
                )
            }
        } else {
            if frame.vector == 14 {
                let cr2: u64;
                unsafe {
                    asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack, preserves_flags))
                };
                super::write("Ring 3 probe #PF CR2=0x");
                super::write_hex_u64(cr2);
                super::write("\r\n");
            }
            0x100 | frame.vector
        };
        resume_ring3_probe(frame, result);
        return frame;
    }
    if frame.vector == 32 || frame.vector == crate::lapic::SCHEDULER_TIMER_VECTOR as u64 {
        let scheduler_tick = frame.vector == crate::lapic::SCHEDULER_TIMER_VECTOR as u64;
        if scheduler_tick {
            crate::lapic::end_of_interrupt();
        } else {
            unsafe { super::out(0x20, 0x20) };
        }
        if !scheduler_tick {
            let ticks = TICKS.fetch_add(1, Ordering::Relaxed) + 1;
            let heartbeat = HEARTBEAT.load(Ordering::Relaxed);
            if heartbeat != LAST_HEARTBEAT.load(Ordering::Relaxed) {
                LAST_HEARTBEAT.store(heartbeat, Ordering::Relaxed);
                LAST_PROGRESS_TICK.store(ticks, Ordering::Relaxed);
            }
            if ticks.saturating_sub(LAST_PROGRESS_TICK.load(Ordering::Relaxed)) > WATCHDOG_TICKS {
                super::record_error(carlio_os::errors::KernelError::WatchdogTimeout);
                super::write("\r\n=== WATCHDOG: SEM PROGRESSO ===\r\n");
                report_registers(frame);
                super::write("Sistema interrompido; diagnóstico serial somente leitura.\r\n");
                stop_cpu();
            }
        }
        // O PIT permanece como preempção confiável do BSP; os APs recebem o
        // mesmo caminho pelo timer local do LAPIC.
        if (scheduler_tick || frame.vector == 32) && frame.cs & 3 == 3 {
            let hook = TIMER_FRAME_SWITCH.load(Ordering::Acquire);
            if hook != 0 {
                let hook: extern "C" fn(usize, *mut UserInterruptFrame) -> *mut UserInterruptFrame =
                    unsafe { core::mem::transmute(hook) };
                let next = hook(cpu, (frame as *mut ExceptionFrame).cast());
                if !next.is_null() {
                    return next.cast();
                }
            }
        }
        return frame;
    }
    super::record_error(carlio_os::errors::KernelError::CpuException);
    super::write("\r\n=== CAPTURA DE EXCECAO CPU ===\r\nVetor: ");
    super::write_u32(frame.vector as u32);
    super::write(" ( ");
    super::write(exception_name(frame.vector));
    super::write(" )\r\nCódigo de erro: 0x");
    super::write_hex_u64(frame.error_code);
    super::write("\r\n");
    report_registers(frame);
    if frame.vector == 14 {
        let cr2: u64;
        unsafe { asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack, preserves_flags)) };
        super::write("CR2 (endereço do page fault): 0x");
        super::write_hex_u64(cr2);
        super::write("\r\n");
    }
    super::write("Sistema interrompido; diagnóstico serial somente leitura.\r\n");
    stop_cpu()
}

#[no_mangle]
extern "C" fn carlio_syscall_dispatch(raw: *const LinuxSyscallFrame) -> u64 {
    let frame = unsafe { &*raw };
    if frame.user_rip < carlio_os::user_memory::PAGE_SIZE
        || frame.user_rip >= carlio_os::elf::USER_TOP
        || frame.user_rsp < carlio_os::user_memory::PAGE_SIZE
        || frame.user_rsp >= carlio_os::elf::USER_TOP
    {
        return (-(carlio_os::linux_abi::EFAULT as i64)) as u64;
    }
    let cpu = crate::lapic::local_id()
        .and_then(crate::cpu_index_for_lapic)
        .unwrap_or(0);
    let arguments = [
        frame.rdi, frame.rsi, frame.rdx, frame.r10, frame.r8, frame.r9,
    ];
    match crate::dispatch_linux_syscall(frame.number, arguments, cpu) {
        Some(carlio_os::linux_abi::SyscallOutcome::Return(value)) => value,
        Some(carlio_os::linux_abi::SyscallOutcome::Exit { status }) => {
            if let Some(next) = crate::process_probe_exit(status as u64) {
                unsafe { enter_user_frame(next) }
            }
            if crate::is_secondary_cpu(cpu) {
                loop {
                    unsafe {
                        asm!("sti; hlt", options(nomem, nostack));
                    }
                }
            }
            if crate::process_probe_is_active() {
                let result = crate::process_probe_result();
                unsafe { carlio_resume_ring3_after_syscall(result) }
            }
            loop {
                unsafe {
                    asm!("sti; hlt", options(nomem, nostack));
                }
            }
        }
        None => (-(carlio_os::linux_abi::ENOSYS as i64)) as u64,
    }
}

fn prepare_ap_idle_resume(frame: &mut ExceptionFrame) {
    frame.rip = core::ptr::addr_of!(scheduler_idle_resume_stub) as u64;
    frame.cs = KERNEL_CODE_SELECTOR as u64;
    frame.rflags = 0x2;
}

fn resume_ring3_probe(frame: &mut ExceptionFrame, result: u64) {
    frame.rax = result;
    frame.rip = core::ptr::addr_of!(carlio_ring3_test_resume) as u64;
    frame.cs = KERNEL_CODE_SELECTOR as u64;
    frame.rflags = 0x2;
    let saved_rsp = unsafe { core::ptr::addr_of!(CARLIO_RING3_SAVED_RSP).read_volatile() };
    let privilege_frame = (frame as *mut ExceptionFrame)
        .cast::<u64>()
        .wrapping_add(core::mem::size_of::<ExceptionFrame>() / 8);
    unsafe {
        privilege_frame.write(saved_rsp);
        privilege_frame.add(1).write(KERNEL_DATA_SELECTOR as u64);
    }
}

fn report_registers(frame: &ExceptionFrame) {
    super::write("RIP: 0x");
    super::write_hex_u64(frame.rip);
    super::write("  CS: 0x");
    super::write_hex_u64(frame.cs);
    super::write("  RFLAGS: 0x");
    super::write_hex_u64(frame.rflags);
    super::write("\r\nRAX=0x");
    super::write_hex_u64(frame.rax);
    super::write(" RBX=0x");
    super::write_hex_u64(frame.rbx);
    super::write(" RCX=0x");
    super::write_hex_u64(frame.rcx);
    super::write(" RDX=0x");
    super::write_hex_u64(frame.rdx);
    super::write("\r\nRSI=0x");
    super::write_hex_u64(frame.rsi);
    super::write(" RDI=0x");
    super::write_hex_u64(frame.rdi);
    super::write(" RBP=0x");
    super::write_hex_u64(frame.rbp);
    super::write("\r\nR8=0x");
    super::write_hex_u64(frame.r8);
    super::write(" R9=0x");
    super::write_hex_u64(frame.r9);
    super::write(" R10=0x");
    super::write_hex_u64(frame.r10);
    super::write(" R11=0x");
    super::write_hex_u64(frame.r11);
    super::write("\r\nR12=0x");
    super::write_hex_u64(frame.r12);
    super::write(" R13=0x");
    super::write_hex_u64(frame.r13);
    super::write(" R14=0x");
    super::write_hex_u64(frame.r14);
    super::write(" R15=0x");
    super::write_hex_u64(frame.r15);
    super::write("\r\n");
}

fn stop_cpu() -> ! {
    loop {
        unsafe { asm!("cli; hlt", options(nomem, nostack)) };
    }
}

pub fn heartbeat() {
    HEARTBEAT.fetch_add(1, Ordering::Relaxed);
}

/// Espera ticks do PIT sem acionar o watchdog durante uma espera de hardware.
pub fn wait_ticks(count: u64) -> bool {
    // Syscall exit and the ring-3 probe may return to kernel code with IF clear.
    // PIT-backed hardware delays must still make progress; this helper is only
    // used by bounded device recovery paths that hold no scheduler/VM locks.
    let flags: u64;
    unsafe { asm!("pushfq; pop {}", out(reg) flags, options(nomem, preserves_flags)) };
    let restore_interrupts_disabled = flags & (1 << 9) == 0;
    if restore_interrupts_disabled {
        unsafe { asm!("sti", options(nomem, nostack)) };
    }
    let deadline = TICKS.load(Ordering::Relaxed).wrapping_add(count);
    let mut elapsed = false;
    for _ in 0..50_000_000 {
        if TICKS.load(Ordering::Relaxed).wrapping_sub(deadline) < (1u64 << 63) {
            elapsed = true;
            break;
        }
        heartbeat();
        core::hint::spin_loop();
    }
    if restore_interrupts_disabled {
        unsafe { asm!("cli", options(nomem, nostack)) };
    }
    elapsed
}

pub unsafe fn start_watchdog() {
    unsafe {
        // Remapeia 8259 PIC e libera apenas IRQ0 no master.
        super::out(0x20, 0x11);
        super::out(0xa0, 0x11);
        super::out(0x21, 0x20);
        super::out(0xa1, 0x28);
        super::out(0x21, 0x04);
        super::out(0xa1, 0x02);
        super::out(0x21, 0x01);
        super::out(0xa1, 0x01);
        super::out(0x21, 0xfe);
        super::out(0xa1, 0xff);
        // PIT channel 0, onda quadrada em 100 Hz.
        let divisor = 1_193_182u32 / 100;
        super::out(0x43, 0x36);
        super::out(0x40, divisor as u8);
        super::out(0x40, (divisor >> 8) as u8);
        TICKS.store(0, Ordering::Relaxed);
        LAST_HEARTBEAT.store(HEARTBEAT.load(Ordering::Relaxed), Ordering::Relaxed);
        LAST_PROGRESS_TICK.store(0, Ordering::Relaxed);
        asm!("sti", options(nomem, nostack));
    }
}

unsafe fn prepare_tss_and_gdt() {
    let ring0_stack_top = unsafe {
        core::ptr::addr_of_mut!(RING0_ENTRY_STACK.0)
            .cast::<u8>()
            .add(16 * 1024) as u64
    };
    let stack_top = unsafe {
        core::ptr::addr_of_mut!(DOUBLE_FAULT_STACK.0)
            .cast::<u8>()
            .add(16 * 1024) as u64
    };
    unsafe {
        core::ptr::addr_of_mut!(TSS.rsp[0]).write_unaligned(ring0_stack_top);
        core::ptr::addr_of_mut!(TSS.ist[0]).write_unaligned(stack_top);
    }
    let tss_base = core::ptr::addr_of!(TSS) as u64;
    let tss_limit = (core::mem::size_of::<TaskStateSegment>() - 1) as u64;
    let low = (tss_limit & 0xffff)
        | ((tss_base & 0x00ff_ffff) << 16)
        | (0x89 << 40)
        | (((tss_limit >> 16) & 0x0f) << 48)
        | (((tss_base >> 24) & 0xff) << 56);
    let high = tss_base >> 32;
    unsafe {
        GDT[1] = 0x00af_9a00_0000_ffff; // código ring 0, 64 bits
        GDT[2] = 0x00cf_9200_0000_ffff; // dados ring 0
        GDT[3] = 0x00cf_f200_0000_ffff; // dados ring 3
        GDT[4] = 0x00af_fa00_0000_ffff; // código ring 3, 64 bits
        GDT[5] = low;
        GDT[6] = high;
    }
    let descriptor = IdtPointer {
        limit: (core::mem::size_of::<[u64; 7]>() - 1) as u16,
        base: core::ptr::addr_of!(GDT) as u64,
    };
    unsafe {
        asm!(
            "lgdt [{descriptor}]",
            "push 0x08",
            "lea rax, [rip + 2f]",
            "push rax",
            "retfq",
            "2:",
            "mov ax, 0x10",
            "mov ds, ax",
            "mov es, ax",
            "mov ss, ax",
            "mov ax, 0x28",
            "ltr ax",
            descriptor = in(reg) &descriptor,
            out("rax") _,
        );
    }
}

fn exception_name(vector: u64) -> &'static str {
    match vector {
        0 => "#DE divisão por zero",
        1 => "#DB depuração",
        2 => "NMI",
        3 => "#BP breakpoint",
        4 => "#OF overflow",
        5 => "#BR limite",
        6 => "#UD instrução inválida",
        7 => "#NM dispositivo indisponível",
        8 => "#DF double fault",
        10 => "#TS segmento inválido",
        11 => "#NP segmento ausente",
        12 => "#SS falha de stack",
        13 => "#GP proteção geral",
        14 => "#PF page fault",
        16 => "#MF x87",
        17 => "#AC alinhamento",
        18 => "#MC machine check",
        19 => "#XM SIMD",
        21 => "#CP proteção de controle",
        29 => "#VC virtualização",
        30 => "#SX segurança",
        _ => "exceção reservada/arquitetural",
    }
}

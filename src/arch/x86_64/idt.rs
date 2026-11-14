//! IDT mínima para capturar faults e exceções de CPU em polling serial.

use core::arch::{asm, global_asm};
use core::sync::atomic::{AtomicU64, Ordering};

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
        movq %r12, %rsp
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

static mut IDT: [IdtEntry; 256] = [IdtEntry::MISSING; 256];
static mut GDT: [u64; 5] = [0; 5];
static mut TSS: TaskStateSegment = TaskStateSegment::EMPTY;
#[repr(align(16))]
struct DoubleFaultStack([u8; 16 * 1024]);
static mut DOUBLE_FAULT_STACK: DoubleFaultStack = DoubleFaultStack([0; 16 * 1024]);

static TICKS: AtomicU64 = AtomicU64::new(0);
static HEARTBEAT: AtomicU64 = AtomicU64::new(0);
static LAST_HEARTBEAT: AtomicU64 = AtomicU64::new(0);
static LAST_PROGRESS_TICK: AtomicU64 = AtomicU64::new(0);
const WATCHDOG_TICKS: u64 = 500; // 5 s em PIT 100 Hz

unsafe extern "C" {
    static exception_stub_table: [usize; 32];
    static watchdog_timer_stub: u8;
}

pub unsafe fn install() {
    unsafe { asm!("cli", options(nomem, nostack)) };
    prepare_tss_and_gdt();
    let selector: u16 = 0x08;
    unsafe {
        let timer_stub = core::ptr::addr_of!(watchdog_timer_stub) as u64;
        IDT[32] = IdtEntry::interrupt_gate(timer_stub, selector, 0);
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

#[no_mangle]
extern "C" fn exception_dispatch(frame: *const ExceptionFrame) {
    let frame = unsafe { &*frame };
    if frame.vector == 32 {
        unsafe { super::out(0x20, 0x20) };
        let ticks = TICKS.fetch_add(1, Ordering::Relaxed) + 1;
        let heartbeat = HEARTBEAT.load(Ordering::Relaxed);
        if heartbeat != LAST_HEARTBEAT.load(Ordering::Relaxed) {
            LAST_HEARTBEAT.store(heartbeat, Ordering::Relaxed);
            LAST_PROGRESS_TICK.store(ticks, Ordering::Relaxed);
            return;
        }
        if ticks.saturating_sub(LAST_PROGRESS_TICK.load(Ordering::Relaxed)) <= WATCHDOG_TICKS {
            return;
        }
        super::record_error(carlio_os::errors::KernelError::WatchdogTimeout);
        super::write("\r\n=== WATCHDOG: SEM PROGRESSO ===\r\n");
        report_registers(frame);
        super::write("Sistema interrompido; diagnóstico serial somente leitura.\r\n");
        stop_cpu();
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
    let deadline = TICKS.load(Ordering::Relaxed).wrapping_add(count);
    for _ in 0..50_000_000 {
        if TICKS.load(Ordering::Relaxed).wrapping_sub(deadline) < (1u64 << 63) {
            return true;
        }
        heartbeat();
        core::hint::spin_loop();
    }
    false
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
    let stack_top = unsafe {
        core::ptr::addr_of_mut!(DOUBLE_FAULT_STACK.0)
            .cast::<u8>()
            .add(16 * 1024) as u64
    };
    unsafe {
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
        GDT[3] = low;
        GDT[4] = high;
    }
    let descriptor = IdtPointer {
        limit: (core::mem::size_of::<[u64; 5]>() - 1) as u16,
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
            "mov ax, 0x18",
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

//! Backend xAPIC para invalidação global de TLB com confirmação remota.
//!
//! O backend usa IPI fixo em 0xf1. Cada CPU remota recarrega seu CR3, envia
//! EOI e só então publica o ACK. Timeout é fatal: continuar poderia reutilizar
//! frames ainda visíveis por uma tradução obsoleta.

use core::arch::asm;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

const IA32_APIC_BASE: u32 = 0x1b;
const APIC_ID: usize = 0x20;
const APIC_EOI: usize = 0xb0;
const APIC_SVR: usize = 0xf0;
const APIC_LVT_TIMER: usize = 0x320;
const APIC_TIMER_INITIAL: usize = 0x380;
const APIC_TIMER_DIVIDE: usize = 0x3e0;
const APIC_ICR_LOW: usize = 0x300;
const APIC_ICR_HIGH: usize = 0x310;
pub const TLB_IPI_VECTOR: u8 = 0xf1;
pub const SCHEDULER_TIMER_VECTOR: u8 = 0x40;
const APIC_ENABLE: u64 = 1 << 11;
const X2APIC_ENABLE: u64 = 1 << 10;
// KVM/firmware pode levar dezenas de milhões de ciclos para entregar um IPI
// quando vários APs iniciam juntos; o ACK permanece obrigatório.
const SPIN_LIMIT: usize = 100_000_000;
pub const MMIO_VIRTUAL_BASE: u64 = 0xffff_c000_0000_0000;

static MMIO_BASE: AtomicUsize = AtomicUsize::new(0);
static EXPECTED_MMIO_PHYSICAL: AtomicU64 = AtomicU64::new(0);
static REMOTE_ACKS: AtomicUsize = AtomicUsize::new(0);
static ONLINE_APS: AtomicUsize = AtomicUsize::new(0);
static SHOOTDOWN_LOCK: AtomicBool = AtomicBool::new(false);
static SHOOTDOWN_COUNT: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LapicError {
    X2ApicActive,
    InvalidBase,
    AcknowledgementTimeout,
}

/// Ativa o xAPIC nesta CPU e o mapeia pelo HHDM. Chamado pelo BSP e por cada AP.
pub fn physical_base() -> Result<u64, LapicError> {
    let low: u32;
    let high: u32;
    unsafe {
        asm!("rdmsr", in("ecx") IA32_APIC_BASE, out("eax") low, out("edx") high, options(nomem, nostack));
    }
    let msr = ((high as u64) << 32) | low as u64;
    if msr & X2APIC_ENABLE != 0 {
        return Err(LapicError::X2ApicActive);
    }
    Ok(msr & 0x000f_ffff_ffff_f000)
}

/// Ativa o xAPIC nesta CPU. O MMIO deve estar mapeado como uncached antes.
pub unsafe fn initialize(mmio_virtual: u64) -> Result<(), LapicError> {
    let mut base_msr: u64;
    unsafe {
        let low: u32;
        let high: u32;
        asm!("rdmsr", in("ecx") IA32_APIC_BASE, out("eax") low, out("edx") high, options(nomem, nostack));
        base_msr = ((high as u64) << 32) | low as u64;
    }
    if base_msr & X2APIC_ENABLE != 0 {
        return Err(LapicError::X2ApicActive);
    }
    if EXPECTED_MMIO_PHYSICAL.load(Ordering::Acquire) == 0
        || base_msr & 0x000f_ffff_ffff_f000 != EXPECTED_MMIO_PHYSICAL.load(Ordering::Acquire)
    {
        return Err(LapicError::InvalidBase);
    }
    base_msr |= APIC_ENABLE;
    let base = mmio_virtual;
    if base == 0 || base & 0xfff != 0 {
        return Err(LapicError::InvalidBase);
    }
    unsafe {
        asm!("wrmsr", in("ecx") IA32_APIC_BASE, in("eax") base_msr as u32,
             in("edx") (base_msr >> 32) as u32, options(nomem, nostack));
    }
    MMIO_BASE.store(base as usize, Ordering::Release);
    let svr = read(APIC_SVR);
    write(APIC_SVR, (svr & !0xff) | 0xff | (1 << 8));
    let _ = read(APIC_ID); // serialize posted MMIO writes
    Ok(())
}

pub fn is_initialized() -> bool {
    MMIO_BASE.load(Ordering::Acquire) != 0
}

/// Retorna o ID local da CPU que está executando esta chamada.
pub fn local_id() -> Option<u32> {
    let base = MMIO_BASE.load(Ordering::Acquire);
    if base == 0 {
        return None;
    }
    Some(unsafe { core::ptr::read_volatile((base + APIC_ID) as *const u32) >> 24 })
}

/// Inicia o timer periódico desta CPU para preempção local.
/// A contagem inicial é deliberadamente comum a todas as CPUs; o clock
/// efetivo depende da plataforma e pode ser calibrado posteriormente.
pub unsafe fn start_scheduler_timer() {
    if !is_initialized() {
        return;
    }
    write(APIC_TIMER_DIVIDE, 0x3);
    write(APIC_LVT_TIMER, (1 << 17) | SCHEDULER_TIMER_VECTOR as u32);
    write(APIC_TIMER_INITIAL, 10_000_000);
}

pub fn end_of_interrupt() {
    write(APIC_EOI, 0);
}

pub fn set_mmio_physical_base(address: u64) -> bool {
    if address == 0 || address & 0xfff != 0 {
        return false;
    }
    EXPECTED_MMIO_PHYSICAL.store(address, Ordering::Release);
    true
}

pub struct LapicShootdown;

impl carlio_os::memory::TlbShootdown for LapicShootdown {
    fn invalidate_all(&self, root: u64, virtual_address: u64) {
        invalidate_all(root, virtual_address);
    }
}

/// Informa quantos APs terminaram a inicialização de IDT e LAPIC.
pub fn set_online_aps(count: usize) {
    ONLINE_APS.store(count, Ordering::Release);
}

/// Handler do vetor dedicado. ACK é publicado após a invalidação e EOI.
#[no_mangle]
pub extern "C" fn handle_tlb_shootdown_ipi() {
    let root: u64;
    let cr4: u64;
    unsafe {
        asm!("mov {}, cr3", out(reg) root, options(nomem, nostack, preserves_flags));
        asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
        if cr4 & (1 << 7) != 0 {
            // CR3 reload alone preserves global translations when PGE is set.
            asm!("mov cr4, {}", in(reg) cr4 & !(1 << 7), options(nostack, preserves_flags));
            asm!("mov cr4, {}", in(reg) cr4, options(nostack, preserves_flags));
        }
        asm!("mov cr3, {}", in(reg) root, options(nostack, preserves_flags));
    }
    write(APIC_EOI, 0);
    REMOTE_ACKS.fetch_add(1, Ordering::Release);
}

/// Invalida localmente e aguarda confirmação explícita de todos os APs ativos.
/// O parâmetro `root` integra o contrato genérico; o CR3 completo é recarregado
/// remotamente, portanto a invalidação cobre todos os espaços de endereçamento.
pub fn invalidate_all(_root: u64, virtual_address: u64) {
    while SHOOTDOWN_LOCK
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        wait_for_shootdown_progress();
    }
    unsafe { asm!("invlpg [{}]", in(reg) virtual_address, options(nostack, preserves_flags)) };
    let expected = ONLINE_APS.load(Ordering::Acquire);
    if expected == 0 {
        SHOOTDOWN_COUNT.fetch_add(1, Ordering::Relaxed);
        SHOOTDOWN_LOCK.store(false, Ordering::Release);
        return;
    }
    REMOTE_ACKS.store(0, Ordering::Release);
    write(APIC_ICR_HIGH, 0);
    // Fixed delivery, shorthand all-excluding-self (bits 19:18 = 11).
    write(APIC_ICR_LOW, TLB_IPI_VECTOR as u32 | (3 << 18));
    let mut spins = 0usize;
    while REMOTE_ACKS.load(Ordering::Acquire) < expected && spins < SPIN_LIMIT {
        wait_for_shootdown_progress();
        spins += 1;
    }
    if spins == SPIN_LIMIT {
        crate::write("LAPIC: ACKs obtidos=");
        crate::write_u32(REMOTE_ACKS.load(Ordering::Relaxed) as u32);
        crate::write(", esperados=");
        crate::write_u32(expected as u32);
        crate::write(".\r\n");
        fatal(LapicError::AcknowledgementTimeout);
    }
    SHOOTDOWN_COUNT.fetch_add(1, Ordering::Relaxed);
    SHOOTDOWN_LOCK.store(false, Ordering::Release);
}

/// A CPU may enter a page-table operation from SYSCALL with IF masked. If it
/// spins on the shootdown lock or waits for remote ACKs with interrupts off,
/// the CPU holding the lock can be waiting for this CPU's IPI and deadlock.
/// Briefly enable interrupts while waiting, then restore the kernel's masked
/// state before touching shared shootdown state again.
#[inline(always)]
pub(crate) fn wait_for_shootdown_progress() {
    let flags: u64;
    unsafe { asm!("pushfq", "pop {}", out(reg) flags) };
    if flags & (1 << 9) != 0 {
        core::hint::spin_loop();
    } else {
        unsafe { asm!("sti; pause; cli", options(nomem, nostack)) };
    }
}

pub fn shootdown_count() -> usize {
    SHOOTDOWN_COUNT.load(Ordering::Acquire)
}

fn read(offset: usize) -> u32 {
    let base = MMIO_BASE.load(Ordering::Acquire);
    if base == 0 {
        fatal(LapicError::InvalidBase);
    }
    unsafe { core::ptr::read_volatile((base + offset) as *const u32) }
}

fn write(offset: usize, value: u32) {
    let base = MMIO_BASE.load(Ordering::Acquire);
    if base == 0 {
        fatal(LapicError::InvalidBase);
    }
    unsafe { core::ptr::write_volatile((base + offset) as *mut u32, value) };
    let _ = read(APIC_ID);
}

fn fatal(error: LapicError) -> ! {
    // Falha fechada: sem ACK não se pode libertar nem reutilizar as páginas.
    crate::write("LAPIC: falha fatal no shootdown remoto: ");
    match error {
        LapicError::AcknowledgementTimeout => crate::write("ACK remoto expirou.\r\n"),
        LapicError::X2ApicActive => crate::write("x2APIC ativo.\r\n"),
        LapicError::InvalidBase => crate::write("mapeamento MMIO inválido.\r\n"),
    }
    loop {
        unsafe { asm!("cli; hlt", options(nomem, nostack)) };
    }
}

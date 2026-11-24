//! The bare-metal Rust target and kernel crypto use software arithmetic.
//! Only scheduled CPL3 tasks use the hardware x87 and SSE registers.

use carlio_os::process::Process;
use carlio_os::sync::IrqSpinLock;
use core::arch::asm;

#[repr(C, align(16))]
struct FxState([u8; 512]);

impl FxState {
    const INITIAL: Self = {
        let mut bytes = [0; 512];
        // x87 control word 0x037f and MXCSR 0x1f80.
        bytes[0] = 0x7f;
        bytes[1] = 0x03;
        bytes[24] = 0x80;
        bytes[25] = 0x1f;
        Self(bytes)
    };
}

struct Slot {
    owner: u64,
    state: FxState,
}

// Positions are owned by the process kernel stack pool. The owner check also
// handles slot reuse after a process has exited or been reaped.
static SLOTS: [IrqSpinLock<Slot>; 64] = [const {
    IrqSpinLock::new(Slot {
        owner: 0,
        state: FxState::INITIAL,
    })
}; 64];

pub(super) fn enable() {
    unsafe {
        asm!(
            "mov {control}, cr0",
            "and {control}, -13", // clear EM and TS
            "or {control}, 34",  // set MP and NE
            "mov cr0, {control}",
            "mov {control}, cr4",
            "and {control}, -262145", // clear OSXSAVE until AVX state is managed
            "or {control}, 1536",    // set OSFXSR and OSXMMEXCPT
            "mov cr4, {control}",
            control = out(reg) _,
            options(nostack),
        );
    }
    reset();
}

pub(super) fn save(process: Process) {
    if let Some(slot) = process
        .kernel_stack_owner
        .and_then(|index| SLOTS.get(index))
    {
        let mut slot = slot.lock();
        unsafe {
            asm!("fxsave64 [{}]", in(reg) &mut slot.state, options(nostack, preserves_flags));
        }
        slot.owner = process.id;
    }
}

pub(super) fn restore(process: Process) {
    if let Some(slot) = process
        .kernel_stack_owner
        .and_then(|index| SLOTS.get(index))
    {
        let slot = slot.lock();
        if slot.owner == process.id {
            restore_state(&slot.state);
            return;
        }
    }
    reset();
}

pub(super) fn reset() {
    restore_state(&FxState::INITIAL);
}

fn restore_state(state: &FxState) {
    let mut empty_env: [u32; 7] = [0x037f, 0, 0xffff, 0, 0, 0, 0];
    unsafe {
        // Some AMD CPUs skip restoring x87 pointers unless an exception is
        // pending. Overwrite the previous task's pointers before FXRSTOR.
        asm!(
            "fnclex",
            "fldenv [{empty}]",
            "fxrstor64 [{state}]",
            empty = in(reg) empty_env.as_mut_ptr(),
            state = in(reg) state,
            options(nostack, preserves_flags),
        );
    }
}

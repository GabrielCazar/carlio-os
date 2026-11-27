//! Background jobs launched by the authenticated management console.
use super::*;
use alloc::{string::String, vec::Vec};
use carlio_os::linux_abi::{EBUSY, EIO, ENOENT, EPERM};
use carlio_os::process::{ProcessSecurityContext, ProcessState};

struct ServiceSlot {
    pid: AtomicU64,
    on_task_stack: AtomicBool,
    stopping: AtomicBool,
    launch_security: IrqSpinLock<Option<ProcessSecurityContext>>,
    arguments: IrqSpinLock<Vec<String>>,
}

static SERVICES: [ServiceSlot; MAX_BOOT_CPUS] = [const {
    ServiceSlot {
        pid: AtomicU64::new(0),
        on_task_stack: AtomicBool::new(false),
        stopping: AtomicBool::new(false),
        launch_security: IrqSpinLock::new(None),
        arguments: IrqSpinLock::new(Vec::new()),
    }
}; MAX_BOOT_CPUS];

pub(super) fn occupied(cpu: usize) -> bool {
    SERVICES[cpu].pid.load(Ordering::Acquire) != 0
}

pub(super) fn owns(pid: u64) -> bool {
    SERVICES
        .iter()
        .any(|slot| slot.pid.load(Ordering::Acquire) == pid)
}

pub(super) fn mark_active(cpu: usize, pid: u64) {
    let slot = &SERVICES[cpu];
    if slot.pid.load(Ordering::Acquire) == pid {
        slot.on_task_stack.store(true, Ordering::Release);
    }
}

pub(super) fn mark_idle(cpu: usize) {
    SERVICES[cpu].on_task_stack.store(false, Ordering::Release);
}

pub(super) fn stop_requested(cpu: usize, pid: u64) -> bool {
    let slot = &SERVICES[cpu];
    slot.pid.load(Ordering::Acquire) == pid && slot.stopping.load(Ordering::Acquire)
}

pub(super) fn publish(cpu: usize, pid: u64) -> Result<(), u16> {
    let slot = &SERVICES[cpu];
    slot.pid.store(pid, Ordering::Release);
    if PROCESS_SCHEDULER.wake(pid).is_err() {
        slot.pid.store(0, Ordering::Release);
        return Err(EIO);
    }
    AP_PROCESS_SCHEDULING.store(true, Ordering::Release);
    Ok(())
}

pub(super) fn start(
    path: &str,
    argv: &[&str],
    security: ProcessSecurityContext,
) -> Result<u64, u16> {
    reap_finished();
    let mask = secondary_cpu_mask();
    let cpu = (0..MAX_BOOT_CPUS)
        .find(|&cpu| mask & (1 << cpu) != 0 && !occupied(cpu))
        .ok_or(EBUSY)?;
    let slot = &SERVICES[cpu];
    *slot.arguments.lock() = argv.iter().map(|arg| String::from(*arg)).collect();
    *slot.launch_security.lock() = Some(security);
    let result = launch_console_process(path, argv, security, Some(cpu));
    if result.is_err() {
        slot.arguments.lock().clear();
        *slot.launch_security.lock() = None;
    }
    result
}

fn terminal(state: ProcessState) -> bool {
    matches!(
        state,
        ProcessState::Exited { .. } | ProcessState::Faulted { .. }
    )
}

fn reap(slot: &ServiceSlot, pid: u64) -> Result<(), u16> {
    let task = PROCESS_SCHEDULER.process(pid).ok_or(EIO)?;
    if !terminal(task.state) || slot.on_task_stack.load(Ordering::Acquire) {
        return Err(EBUSY);
    }
    reap_process(pid).map_err(|_| EIO)?;
    slot.arguments.lock().clear();
    *slot.launch_security.lock() = None;
    slot.stopping.store(false, Ordering::Relaxed);
    slot.pid.store(0, Ordering::Release);
    Ok(())
}

pub(super) fn reap_finished() {
    for slot in &SERVICES {
        let pid = slot.pid.load(Ordering::Acquire);
        if pid == 0 {
            continue;
        }
        if slot.stopping.load(Ordering::Acquire) {
            let _ = PROCESS_SCHEDULER.destroy(pid, -15);
        }
        let _ = reap(slot, pid);
    }
}

fn authorize(pid: u64, uid: u32) -> Result<(&'static ServiceSlot, ProcessSecurityContext), u16> {
    let slot = SERVICES
        .iter()
        .find(|slot| pid != 0 && slot.pid.load(Ordering::Acquire) == pid)
        .ok_or(ENOENT)?;
    let security = (*slot.launch_security.lock()).ok_or(ENOENT)?;
    if uid != 0 && uid != security.credentials.uid {
        return Err(EPERM);
    }
    Ok((slot, security))
}

pub(super) fn restart(pid: u64, uid: u32) -> Result<u64, u16> {
    let (slot, security) = authorize(pid, uid)?;
    let arguments = slot.arguments.lock().clone();
    let argv: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let path = argv.first().copied().ok_or(EIO)?;
    stop(pid, uid)?;
    start(path, &argv, security)
}

pub(super) fn stop(pid: u64, uid: u32) -> Result<(), u16> {
    let (slot, _) = authorize(pid, uid)?;
    slot.stopping.store(true, Ordering::Release);
    for _ in 0..100 {
        // Ready and blocked tasks can be cancelled directly. Running tasks
        // are stopped by the local timer before returning to user mode.
        let _ = PROCESS_SCHEDULER.destroy(pid, -15);
        match reap(slot, pid) {
            Err(EBUSY) => {}
            result => return result,
        }
        if !idt::wait_ticks(1) {
            break;
        }
    }
    Err(EBUSY)
}

pub(super) fn list(uid: u32) {
    for slot in &SERVICES {
        let pid = slot.pid.load(Ordering::Acquire);
        let Some(task) = PROCESS_SCHEDULER.process(pid) else {
            continue;
        };
        let Some(security) = *slot.launch_security.lock() else {
            continue;
        };
        if uid != 0 && uid != security.credentials.uid {
            continue;
        }
        write("Serviço pid=");
        write_u64(pid);
        write(match task.state {
            ProcessState::Ready => " pronto\r\n",
            ProcessState::Running { .. } => " em execução\r\n",
            ProcessState::Blocked => " bloqueado\r\n",
            ProcessState::Exited { .. } => " encerrado\r\n",
            ProcessState::Faulted { .. } => " falhou\r\n",
            _ => " liberando recursos\r\n",
        });
    }
}

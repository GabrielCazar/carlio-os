//! Primitivas básicas de sincronização para estruturas compartilhadas.
//!
//! `SpinLock` pode proteger estado compartilhado entre CPUs, mas não desativa
//! interrupções nem preempção. O kernel não deve adquiri-lo em contexto de
//! interrupção nem habilitar preempção enquanto o mantém até integrar essas
//! políticas ao escalonador.

#[cfg(target_os = "none")]
use core::arch::asm;
use core::cell::UnsafeCell;
use core::hint::spin_loop;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

pub struct SpinLock<T> {
    held: AtomicBool,
    value: UnsafeCell<T>,
}

unsafe impl<T: Send> Send for SpinLock<T> {}
unsafe impl<T: Send> Sync for SpinLock<T> {}

impl<T> SpinLock<T> {
    pub const fn new(value: T) -> Self {
        Self {
            held: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    pub fn lock(&self) -> SpinLockGuard<'_, T> {
        while self
            .held
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            while self.held.load(Ordering::Relaxed) {
                spin_loop();
            }
        }
        SpinLockGuard { lock: self }
    }

    pub fn try_lock(&self) -> Option<SpinLockGuard<'_, T>> {
        self.held
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| SpinLockGuard { lock: self })
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.value.get_mut()
    }
}

pub struct SpinLockGuard<'a, T> {
    lock: &'a SpinLock<T>,
}

impl<T> Deref for SpinLockGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.value.get() }
    }
}

impl<T> DerefMut for SpinLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // Exclusividade garantida pelo estado adquirido com Ordering::Acquire.
        unsafe { &mut *self.lock.value.get() }
    }
}

impl<T> Drop for SpinLockGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.held.store(false, Ordering::Release);
    }
}

/// Spin lock for state shared with local interrupt handlers. It disables
/// interrupts before acquiring the inner lock and restores the prior IF state
/// only after releasing it. It does not provide scheduler preemption control
/// beyond the local IRQ mask and still requires bounded critical sections.
pub struct IrqSpinLock<T> {
    inner: SpinLock<T>,
}

impl<T> IrqSpinLock<T> {
    pub const fn new(value: T) -> Self {
        Self {
            inner: SpinLock::new(value),
        }
    }

    pub fn lock(&self) -> IrqSpinLockGuard<'_, T> {
        let interrupts_were_enabled = local_irq_save_disable();
        let guard = self.inner.lock();
        IrqSpinLockGuard {
            guard: Some(guard),
            interrupts_were_enabled,
        }
    }

    pub fn try_lock(&self) -> Option<IrqSpinLockGuard<'_, T>> {
        let interrupts_were_enabled = local_irq_save_disable();
        match self.inner.try_lock() {
            Some(guard) => Some(IrqSpinLockGuard {
                guard: Some(guard),
                interrupts_were_enabled,
            }),
            None => {
                local_irq_restore(interrupts_were_enabled);
                None
            }
        }
    }
}

pub struct IrqSpinLockGuard<'a, T> {
    guard: Option<SpinLockGuard<'a, T>>,
    interrupts_were_enabled: bool,
}

impl<T> Deref for IrqSpinLockGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.guard.as_deref().expect("guard is live")
    }
}

impl<T> DerefMut for IrqSpinLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard.as_deref_mut().expect("guard is live")
    }
}

impl<T> Drop for IrqSpinLockGuard<'_, T> {
    fn drop(&mut self) {
        drop(self.guard.take());
        local_irq_restore(self.interrupts_were_enabled);
    }
}

#[cfg(target_os = "none")]
fn local_irq_save_disable() -> bool {
    let flags: u64;
    unsafe {
        asm!("pushfq", "pop {}", "cli", out(reg) flags);
    }
    flags & (1 << 9) != 0
}

#[cfg(not(target_os = "none"))]
fn local_irq_save_disable() -> bool {
    false
}

#[cfg(target_os = "none")]
fn local_irq_restore(was_enabled: bool) {
    if was_enabled {
        unsafe { asm!("sti", options(nomem, nostack)) };
    }
}

#[cfg(not(target_os = "none"))]
fn local_irq_restore(_was_enabled: bool) {}

#[cfg(test)]
mod tests {
    use super::{IrqSpinLock, SpinLock};
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn serializes_mutations_across_host_threads() {
        const WORKERS: usize = 8;
        const INCREMENTS: usize = 4_000;
        let counter = Arc::new(SpinLock::new(0usize));
        let workers: Vec<_> = (0..WORKERS)
            .map(|_| {
                let counter = Arc::clone(&counter);
                thread::spawn(move || {
                    for _ in 0..INCREMENTS {
                        *counter.lock() += 1;
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(*counter.lock(), WORKERS * INCREMENTS);
    }

    #[test]
    fn try_lock_reports_contention_and_releases_on_drop() {
        let lock = SpinLock::new(5);
        let guard = lock.try_lock().unwrap();
        assert!(lock.try_lock().is_none());
        drop(guard);
        assert_eq!(*lock.try_lock().unwrap(), 5);
    }

    #[test]
    fn irq_lock_releases_inner_lock_before_restoring_interrupt_state() {
        let lock = IrqSpinLock::new(5);
        {
            let mut guard = lock.lock();
            *guard += 1;
            assert!(lock.try_lock().is_none());
        }
        assert_eq!(*lock.try_lock().unwrap(), 6);
    }

    #[test]
    fn irq_lock_serializes_host_threads() {
        const WORKERS: usize = 4;
        const INCREMENTS: usize = 1_000;
        let lock = Arc::new(IrqSpinLock::new(0usize));
        let workers: Vec<_> = (0..WORKERS)
            .map(|_| {
                let lock = Arc::clone(&lock);
                thread::spawn(move || {
                    for _ in 0..INCREMENTS {
                        *lock.lock() += 1;
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(*lock.lock(), WORKERS * INCREMENTS);
    }
}

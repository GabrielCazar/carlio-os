//! Exclusão mínima para consumir o único registrador RX da UART.
use core::sync::atomic::{AtomicBool, Ordering};

pub struct SerialRxGate {
    owned: AtomicBool,
}

pub struct SerialRxClaim<'a> {
    gate: &'a SerialRxGate,
}

impl SerialRxGate {
    pub const fn new() -> Self {
        Self {
            owned: AtomicBool::new(false),
        }
    }

    /// Reserva brevemente a checagem final do status e a leitura do byte RX.
    pub fn try_claim(&self) -> Option<SerialRxClaim<'_>> {
        self.owned
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| SerialRxClaim { gate: self })
    }
}

impl Default for SerialRxGate {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SerialRxClaim<'_> {
    fn drop(&mut self) {
        self.gate.owned.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::SerialRxGate;

    #[test]
    fn rejects_a_second_claim_until_the_current_reader_releases() {
        let gate = SerialRxGate::new();
        let owner = gate.try_claim().expect("a UART RX claim must be available");

        assert!(gate.try_claim().is_none());

        drop(owner);
        assert!(gate.try_claim().is_some());
    }
}

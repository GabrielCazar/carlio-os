//! Registro fixo e sem alocação para erros do kernel.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum KernelError {
    SerialReadTimeout = 1,
    SerialWriteTimeout = 2,
    InputTooLong = 3,
    DeviceNotFound = 10,
    UnsupportedDevice = 11,
    DeviceInitFailed = 12,
    IoTimeout = 13,
    ControllerError = 14,
    BootInfoMissing = 20,
    CpuException = 21,
    WatchdogTimeout = 22,
    LockdownDenied = 23,
    KernelPanic = 255,
}

impl KernelError {
    pub const fn message(self) -> &'static str {
        match self {
            Self::SerialReadTimeout => "tempo esgotado ao receber pela serial",
            Self::SerialWriteTimeout => "tempo esgotado ao transmitir pela serial",
            Self::InputTooLong => "entrada excedeu o limite do terminal",
            Self::DeviceNotFound => "dispositivo nao encontrado",
            Self::UnsupportedDevice => "dispositivo sem suporte",
            Self::DeviceInitFailed => "falha ao inicializar dispositivo",
            Self::IoTimeout => "tempo esgotado durante I/O",
            Self::ControllerError => "controlador informou erro",
            Self::BootInfoMissing => "informacao de memoria do bootloader ausente",
            Self::CpuException => "excecao/fault de CPU capturado pela IDT",
            Self::LockdownDenied => "operacao bloqueada pela politica lockdown",
            Self::WatchdogTimeout => "watchdog detectou ausencia de progresso do kernel",
            Self::KernelPanic => "falha critica do kernel; modo de recuperacao",
        }
    }
}

#[derive(Clone, Copy)]
pub struct ErrorLog<const N: usize> {
    entries: [Option<KernelError>; N],
    next: usize,
    len: usize,
    total: u32,
}

impl<const N: usize> ErrorLog<N> {
    pub const fn new() -> Self {
        Self {
            entries: [None; N],
            next: 0,
            len: 0,
            total: 0,
        }
    }

    pub fn record(&mut self, error: KernelError) {
        if N == 0 {
            self.total = self.total.saturating_add(1);
            return;
        }
        self.entries[self.next] = Some(error);
        self.next = (self.next + 1) % N;
        self.len = (self.len + 1).min(N);
        self.total = self.total.saturating_add(1);
    }

    pub const fn total(&self) -> u32 {
        self.total
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Retorna erros da entrada mais recente para trás.
    pub fn recent(&self, index: usize) -> Option<KernelError> {
        if index >= self.len || N == 0 {
            return None;
        }
        let slot = (self.next + N - 1 - index) % N;
        self.entries[slot]
    }
}

impl<const N: usize> Default for ErrorLog<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_recent_errors_and_counts_overwrites() {
        let mut log = ErrorLog::<2>::new();
        log.record(KernelError::IoTimeout);
        log.record(KernelError::ControllerError);
        log.record(KernelError::DeviceNotFound);
        assert_eq!(log.total(), 3);
        assert_eq!(log.len(), 2);
        assert_eq!(log.recent(0), Some(KernelError::DeviceNotFound));
        assert_eq!(log.recent(1), Some(KernelError::ControllerError));
        assert_eq!(log.recent(2), None);
    }
}

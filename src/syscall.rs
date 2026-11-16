//! ABI inicial de chamadas de sistema x86_64 do CarlioOS.
//!
//! Registradores: RAX = número; RDI, RSI, RDX, R10, R8 e R9 = argumentos;
//! retorno em RAX. Os números e retornos negativos são estáveis para userland.

pub const SYS_ABI_VERSION: u64 = 0;
pub const SYS_GETPID: u64 = 1;
pub const SYS_UPTIME_TICKS: u64 = 2;
pub const SYS_EXIT: u64 = 3;
/// Número privado para a prova de rotinas do kernel durante o smoke CPL3.
pub const SYS_KERNEL_ROUTINE_PROBE: u64 = 0xfeed;

pub const ABI_VERSION: u64 = 1;
pub const ERROR_NO_SYSCALL: u64 = (-38i64) as u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyscallContext {
    pub process_id: u64,
    pub uptime_ticks: u64,
}

/// Despacha chamadas sem ponteiros de usuário; SYS_EXIT é tratado pelo runtime.
/// Chamadas futuras que recebem buffers precisam validar todo o intervalo
/// contra o espaço de endereços do processo antes de acessá-lo.
pub const fn dispatch(number: u64, _arguments: [u64; 6], context: SyscallContext) -> u64 {
    match number {
        SYS_ABI_VERSION => ABI_VERSION,
        SYS_GETPID => context.process_id,
        SYS_UPTIME_TICKS => context.uptime_ticks,
        _ => ERROR_NO_SYSCALL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTEXT: SyscallContext = SyscallContext {
        process_id: 7,
        uptime_ticks: 1234,
    };

    #[test]
    fn dispatches_version_and_read_only_process_information() {
        assert_eq!(dispatch(SYS_ABI_VERSION, [0; 6], CONTEXT), ABI_VERSION);
        assert_eq!(dispatch(SYS_GETPID, [0; 6], CONTEXT), 7);
        assert_eq!(dispatch(SYS_UPTIME_TICKS, [0; 6], CONTEXT), 1234);
    }

    #[test]
    fn rejects_unknown_numbers_with_enosys() {
        assert_eq!(dispatch(u64::MAX, [0; 6], CONTEXT), ERROR_NO_SYSCALL);
    }
}

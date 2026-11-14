#![cfg_attr(target_os = "none", no_std)]

/// A deliberately small command parser shared by the kernel shell and host tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Help,
    About,
    User,
    Permissions,
    Clear,
    Halt,
    Errors,
    Unknown,
}

pub fn parse_command(line: &str) -> Command {
    match line.trim() {
        "ajuda" => Command::Help,
        "sobre" => Command::About,
        "usuario" => Command::User,
        "permissoes" => Command::Permissions,
        "limpar" => Command::Clear,
        "parar" => Command::Halt,
        "erros" => Command::Errors,
        _ => Command::Unknown,
    }
}

pub const FILESYSTEM_NAME: &str = "Hermione";
pub const HERM_FS_CLI: &str = "herm-fs";

/// Describes policy intent only. It does not isolate code while the kernel runs
/// in ring 0; enforcement belongs to the future process/syscall layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Principal {
    Guest,
    Kernel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    ReadStatus,
    ChangeSystemState,
}

pub const fn allowed(principal: Principal, capability: Capability) -> bool {
    match (principal, capability) {
        (Principal::Guest, Capability::ReadStatus) => true,
        (Principal::Kernel, _) => true,
        (Principal::Guest, Capability::ChangeSystemState) => false,
    }
}

pub mod storage;
pub use storage as herm_fs;
pub mod errors;
pub mod hermione;
pub mod memory;
pub mod pci;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_commands_and_whitespace() {
        assert_eq!(parse_command(" ajuda  "), Command::Help);
        assert_eq!(parse_command("permissoes"), Command::Permissions);
        assert_eq!(parse_command("parar"), Command::Halt);
    }

    #[test]
    fn rejects_unknown_and_empty_commands() {
        assert_eq!(parse_command(""), Command::Unknown);
        assert_eq!(parse_command("apagar sistema"), Command::Unknown);
    }

    #[test]
    fn herm_fs_cli_points_to_hermione_module() {
        assert_eq!(HERM_FS_CLI, "herm-fs");
        assert_eq!(FILESYSTEM_NAME, "Hermione");
        assert_eq!(herm_fs::BLOCK_SIZE, storage::BLOCK_SIZE);
    }

    #[test]
    fn guest_cannot_change_system_state() {
        assert!(allowed(Principal::Guest, Capability::ReadStatus));
        assert!(!allowed(Principal::Guest, Capability::ChangeSystemState));
        assert!(allowed(Principal::Kernel, Capability::ChangeSystemState));
    }
}

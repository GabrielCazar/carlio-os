use carlio_os::{FILESYSTEM_NAME, HERM_FS_CLI};
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let result = match args.next().as_deref() {
        None | Some("help") | Some("--help") | Some("-h") => Ok(help().to_owned()),
        Some("status") if args.next().is_none() => Ok(status()),
        Some(command) => Err(format!("comando ou argumento desconhecido: {command}")),
    };

    match result {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{HERM_FS_CLI}: {error}\nUse '{HERM_FS_CLI} --help' para ver os comandos.");
            ExitCode::from(2)
        }
    }
}

fn help() -> &'static str {
    "Ferramenta de linha de comando do filesystem Hermione.\n\nUso: herm-fs <comando>\n\nComandos:\n  status    mostra o estado atual do suporte ao filesystem\n  help      mostra esta ajuda"
}

fn status() -> String {
    format!("Filesystem: {FILESYSTEM_NAME}\nEstado: formato de volume e camada de bloco experimentais disponíveis; nenhum volume montado nesta execução.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_names_the_tool_and_filesystem() {
        let output = help();
        assert!(output.contains(HERM_FS_CLI));
        assert!(output.contains(FILESYSTEM_NAME));
    }

    #[test]
    fn status_does_not_claim_a_mounted_volume() {
        let output = status();
        assert!(output.contains(FILESYSTEM_NAME));
        assert!(output.contains("nenhum volume montado nesta execução"));
    }
}

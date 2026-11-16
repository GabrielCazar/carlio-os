use carlio_os::log_report::{generate_markdown, view_markdown};
use std::{env, fs, path::Path, process::ExitCode};

const TOOL: &str = "carlio-logs";

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let result = match args.next().as_deref() {
        None | Some("help") | Some("--help") | Some("-h") => {
            println!("{}", help());
            return ExitCode::SUCCESS;
        }
        Some("gerar") => match (args.next(), args.next(), args.next()) {
            (Some(directory), Some(output), None) => generate_markdown(Path::new(&directory))
                .and_then(|markdown| fs::write(&output, markdown).map(|()| output))
                .map(|path| format!("Relatório Markdown salvo em {path}"))
                .map_err(|error| error.to_string()),
            _ => Err("uso: carlio-logs gerar <diretório-de-logs> <saída.md>".into()),
        },
        Some("ver") => match (args.next(), args.next()) {
            (Some(path), None) => view_markdown(Path::new(&path), 22)
                .map(|()| "Visualização encerrada.".into())
                .map_err(|error| error.to_string()),
            _ => Err("uso: carlio-logs ver <arquivo.md>".into()),
        },
        Some(command) => Err(format!("comando desconhecido: {command}")),
    };
    match result {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{TOOL}: {error}\nUse '{TOOL} --help' para ver os comandos.");
            ExitCode::from(2)
        }
    }
}

fn help() -> &'static str {
    "Relatórios Markdown e visualizador nativo Rust para capturas do CarlioOS.\n\nUso:\n  carlio-logs gerar <diretório-de-logs> <saída.md>\n  carlio-logs ver <arquivo.md>\n\nO gerador resume arquivos .log e .txt em tabela e gráficos de barras textuais.\nO visualizador pagina o Markdown no terminal; Enter/n avança, p volta, q sai."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_documents_generator_and_native_viewer() {
        let text = help();
        assert!(text.contains("gerar <diretório-de-logs>"));
        assert!(text.contains("ver <arquivo.md>"));
        assert!(text.contains("visualizador nativo Rust"));
    }
}

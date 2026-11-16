//! Geração e leitura paginada de relatórios Markdown a partir de capturas.
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LogSummary {
    pub name: String,
    pub bytes: u64,
    pub tests_passed: usize,
    pub tests_failed: usize,
    pub free_frames: Option<u64>,
    pub vm_passed: bool,
    pub status: &'static str,
    pub highlights: Vec<String>,
}

pub fn summarize(name: &str, contents: &str) -> LogSummary {
    let mut summary = LogSummary {
        name: name.to_owned(),
        bytes: contents.len() as u64,
        ..LogSummary::default()
    };
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("test ") && trimmed.ends_with(" ... ok") {
            summary.tests_passed += 1;
        }
        if trimmed.starts_with("test ") && trimmed.ends_with(" ... FAILED") {
            summary.tests_failed += 1;
        }
        if trimmed.starts_with("test result:") {
            if let Some((count, _)) = trimmed.split_once(" passed") {
                if let Some(value) = count
                    .rsplit(|c: char| !c.is_ascii_digit())
                    .next()
                    .and_then(|v| v.parse::<usize>().ok())
                {
                    summary.tests_passed = summary.tests_passed.max(value);
                }
            }
            if let Some((count, _)) = trimmed.split_once(" failed") {
                if let Some(value) = count
                    .rsplit(|c: char| !c.is_ascii_digit())
                    .next()
                    .and_then(|v| v.parse::<usize>().ok())
                {
                    summary.tests_failed = summary.tests_failed.max(value);
                }
            }
        }
        if let Some(value) = trimmed.split("frames livres abaixo de 4 GiB=").nth(1) {
            summary.free_frames = value
                .split(|c: char| !c.is_ascii_digit())
                .next()
                .and_then(|v| v.parse().ok());
        }
        if trimmed.starts_with("VM:") && trimmed.contains("passaram") {
            summary.vm_passed = true;
        }
        let important = trimmed.starts_with("VM:")
            || trimmed.starts_with("Allocator físico:")
            || trimmed.starts_with("test result:")
            || trimmed.contains("Falha no teste")
            || trimmed.contains("panicked at")
            || trimmed.contains("ERROR")
            || trimmed.contains("FAILED");
        if important
            && !summary.highlights.iter().any(|item| item == trimmed)
            && summary.highlights.len() < 12
        {
            summary.highlights.push(trimmed.to_owned());
        }
    }
    summary.status = if summary.tests_failed > 0
        || contents.contains("Falha no teste QEMU")
        || contents.contains("panicked at")
    {
        "FALHOU"
    } else if summary.tests_passed > 0
        || summary.vm_passed
        || contents.contains("Teste QEMU aprovado")
    {
        "PASSOU"
    } else {
        "SEM MARCADOR"
    };
    summary
}

pub fn collect_logs(directory: &Path) -> io::Result<Vec<(LogSummary, PathBuf)>> {
    let mut paths = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("log" | "txt")
            )
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            // Capturas seriais podem conter bytes corrompidos ou escapes de
            // terminal. Preserve o relatório e substitua somente UTF-8 inválido.
            let bytes = fs::read(&path)?;
            let contents = String::from_utf8_lossy(&bytes);
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("log sem nome");
            Ok((summarize(name, &contents), path))
        })
        .collect()
}

pub fn generate_markdown(directory: &Path) -> io::Result<String> {
    let logs = collect_logs(directory)?;
    let passed = logs
        .iter()
        .filter(|(log, _)| log.status == "PASSOU")
        .count();
    let failed = logs
        .iter()
        .filter(|(log, _)| log.status == "FALHOU")
        .count();
    let unknown = logs.len().saturating_sub(passed + failed);
    let mut report = String::from("# Painel de logs do CarlioOS\n\n");
    report.push_str(&format!(
        "Diretório analisado: `{}`\n\n",
        directory.display()
    ));
    report.push_str("Este painel agrega execuções de várias datas e versões do código. `FALHOU` ou `SEM MARCADOR` descreve apenas o arquivo correspondente; não é um veredito do estado atual. Ausência de marcador nunca é contada como êxito.\n\n");
    report.push_str("## Resumo\n\n| Indicador | Total | Gráfico |\n|---|---:|---|\n");
    let max = logs.len().max(1);
    for (label, count, glyph) in [
        ("Capturas", logs.len(), "█"),
        ("Aprovadas", passed, "🟩"),
        ("Com falha", failed, "🟥"),
        ("Sem marcador", unknown, "⬜"),
    ] {
        let bar = glyph.repeat(count.saturating_mul(24).div_ceil(max));
        report.push_str(&format!("| {label} | {count} | {bar} |\n"));
    }
    report.push_str("\n## Tabela de validações\n\n| Arquivo | Estado | Testes OK | Testes com falha | Frames livres | VM | Bytes |\n|---|---|---:|---:|---:|---|---:|\n");
    for (log, _) in &logs {
        report.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            escape_cell(&log.name),
            log.status,
            log.tests_passed,
            log.tests_failed,
            log.free_frames
                .map(|n| n.to_string())
                .unwrap_or_else(|| "—".into()),
            if log.vm_passed { "passou" } else { "—" },
            log.bytes
        ));
    }
    report.push_str("\n## Eventos relevantes por arquivo\n");
    for (log, _) in &logs {
        report.push_str(&format!("\n### `{}`\n\n", log.name));
        if log.highlights.is_empty() {
            report.push_str("Sem linhas de resultado reconhecidas.\n");
        } else {
            report.push_str("| Evento |\n|---|\n");
            for line in &log.highlights {
                report.push_str(&format!("| {} |\n", escape_cell(line)));
            }
        }
    }
    report.push_str("\n---\nGerado por `carlio-logs` em Rust. O estado é inferido pelos marcadores encontrados nos arquivos.\n");
    Ok(report)
}

pub fn view_markdown(path: &Path, page_lines: usize) -> io::Result<()> {
    let content = fs::read_to_string(path)?;
    let lines = content.lines().collect::<Vec<_>>();
    let page_lines = page_lines.max(1);
    let mut start = 0;
    let stdin = io::stdin();
    let mut input = String::new();
    while start < lines.len() {
        print!("\x1b[2J\x1b[H");
        for line in lines.iter().skip(start).take(page_lines) {
            println!("{line}");
        }
        start = (start + page_lines).min(lines.len());
        if start < lines.len() {
            print!("\n[Enter/n: próxima · p: voltar · q: sair] ");
            io::stdout().flush()?;
            input.clear();
            stdin.read_line(&mut input)?;
            match input.trim() {
                "q" => break,
                "p" => start = start.saturating_sub(page_lines),
                _ => {}
            }
        }
    }
    Ok(())
}

fn escape_cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_host_and_qemu_markers() {
        let log = summarize("boot.log", "test a ... ok\ntest b ... ok\ntest result: ok. 2 passed; 0 failed\nAllocator físico: frames livres abaixo de 4 GiB=123; alocação, escrita, leitura e liberação passaram.\nVM: mapeamento, acesso, invalidação TLB, desmapeamento e liberação de tabelas passaram.\n");
        assert_eq!(log.status, "PASSOU");
        assert_eq!(log.tests_passed, 2);
        assert_eq!(log.free_frames, Some(123));
        assert!(log.vm_passed);
    }

    #[test]
    fn failure_marker_overrides_success_and_markdown_escapes_cells() {
        let log = summarize("bad|run.txt", "test x ... FAILED\n");
        assert_eq!(log.status, "FALHOU");
        assert_eq!(escape_cell(&log.name), "bad\\|run.txt");
    }

    #[test]
    fn generator_builds_a_markdown_table_and_text_chart() {
        let dir = std::env::temp_dir().join(format!("carlio-logs-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("boot.log"), "Teste QEMU aprovado (tcg)\n").unwrap();
        let markdown = generate_markdown(&dir).unwrap();
        assert!(markdown.contains("| Arquivo | Estado |"));
        assert!(markdown.contains("| Aprovadas | 1 |"));
        assert!(markdown.contains("boot.log | PASSOU"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn generator_recovers_from_invalid_utf8_in_serial_capture() {
        let dir =
            std::env::temp_dir().join(format!("carlio-logs-utf8-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("damaged.log"),
            b"Teste QEMU aprovado (tcg)\nbyte invalido: \xff\n",
        )
        .unwrap();
        let markdown = generate_markdown(&dir).unwrap();
        assert!(markdown.contains("damaged.log | PASSOU"));
        fs::remove_dir_all(dir).unwrap();
    }
}

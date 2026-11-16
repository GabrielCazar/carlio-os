use carlio_os::hermione::{
    BlockDevice, FileInfo, FsError, HermioneVolume, ScrubReport, MAX_VOLUME_SECTORS, SECTOR_SIZE,
};
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    process::ExitCode,
};

struct ImageDevice {
    file: File,
    sectors: u64,
}

impl ImageDevice {
    fn open(path: &Path) -> Result<Self, String> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        let bytes = file.metadata().map_err(|e| e.to_string())?.len();
        if bytes % SECTOR_SIZE as u64 != 0 {
            return Err("o arquivo-imagem não tem tamanho múltiplo de 512 bytes".into());
        }
        Ok(Self {
            file,
            sectors: bytes / SECTOR_SIZE as u64,
        })
    }
}

impl BlockDevice for ImageDevice {
    fn sector_count(&self) -> u64 {
        self.sectors
    }
    fn read_sector(&mut self, lba: u64, out: &mut [u8; SECTOR_SIZE]) -> Result<(), FsError> {
        if lba >= self.sectors {
            return Err(FsError::Device);
        }
        self.file
            .seek(SeekFrom::Start(lba * SECTOR_SIZE as u64))
            .map_err(|_| FsError::Device)?;
        self.file.read_exact(out).map_err(|_| FsError::Device)
    }
    fn write_sector(&mut self, lba: u64, data: &[u8; SECTOR_SIZE]) -> Result<(), FsError> {
        if lba >= self.sectors {
            return Err(FsError::Device);
        }
        self.file
            .seek(SeekFrom::Start(lba * SECTOR_SIZE as u64))
            .map_err(|_| FsError::Device)?;
        self.file.write_all(data).map_err(|_| FsError::Device)
    }
    fn flush(&mut self) -> Result<(), FsError> {
        self.file.sync_all().map_err(|_| FsError::Device)
    }
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let result = match args.next().as_deref() {
        None | Some("help") | Some("--help") | Some("-h") => {
            println!("{}", help());
            return ExitCode::SUCCESS;
        }
        Some("status") if args.next().is_none() => Ok(status()),
        Some("criar") => match (args.next(), args.next(), args.next()) {
            (Some(path), Some(sectors), None) => sectors
                .parse::<u64>()
                .map_err(|_| "quantidade de setores inválida".to_owned())
                .and_then(|count| create_image(Path::new(&path), count)),
            _ => Err("uso: herm-fs criar <imagem-nova> <setores>".into()),
        },
        Some("listar") => match (args.next(), args.next()) {
            (Some(path), None) => list_image(Path::new(&path)),
            _ => Err("uso: herm-fs listar <imagem>".into()),
        },
        Some("gravar") => match (args.next(), args.next(), args.next(), args.next()) {
            (Some(image), Some(name), Some(source), None) => {
                write_image(Path::new(&image), &name, Path::new(&source))
            }
            _ => Err("uso: herm-fs gravar <imagem> <nome> <arquivo-origem>".into()),
        },
        Some("ler") => match (args.next(), args.next(), args.next(), args.next()) {
            (Some(image), Some(name), Some(destination), None) => {
                read_image(Path::new(&image), &name, Path::new(&destination))
            }
            _ => Err("uso: herm-fs ler <imagem> <nome> <arquivo-destino>".into()),
        },
        Some("scrub") => match (args.next(), args.next()) {
            (Some(path), None) => scrub_image(Path::new(&path)),
            _ => Err("uso: herm-fs scrub <imagem>".into()),
        },
        Some(command) => Err(format!("comando ou argumento desconhecido: {command}")),
    };
    match result {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("herm-fs: {error}\nUse 'herm-fs --help' para ver os comandos.");
            ExitCode::from(2)
        }
    }
}

fn create_image(path: &Path, sectors: u64) -> Result<String, String> {
    if sectors <= 5 {
        return Err("o volume precisa de pelo menos 6 setores".into());
    }
    if sectors > MAX_VOLUME_SECTORS {
        return Err("o formato limita o volume a 2 TiB".into());
    }
    let bytes = sectors
        .checked_mul(SECTOR_SIZE as u64)
        .ok_or("tamanho excede limite")?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("não foi possível criar sem sobrescrever: {e}"))?;
    if let Err(error) = file.set_len(bytes) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(error.to_string());
    }
    drop(file);
    let device = ImageDevice::open(path)?;
    if let Err(error) = HermioneVolume::format(device) {
        let _ = fs::remove_file(path);
        return Err(format!("formatação falhou: {error:?}"));
    }
    Ok(format!(
        "Volume Hermione v3 criado: {} setores ({} bytes).",
        sectors, bytes
    ))
}

fn list_image(path: &Path) -> Result<String, String> {
    let mut volume = HermioneVolume::mount(ImageDevice::open(path)?)
        .map_err(|e| format!("montagem falhou: {e:?}"))?;
    let mut files: [Option<FileInfo>; carlio_os::hermione::MAX_FILES] =
        [None; carlio_os::hermione::MAX_FILES];
    let count = volume
        .list(&mut files)
        .map_err(|e| format!("listagem falhou: {e:?}"))?;
    let mut output = format!("Arquivos: {count}/{}\n", files.len());
    for info in files.iter().flatten() {
        let name = core::str::from_utf8(&info.name)
            .unwrap_or("<nome inválido>")
            .trim_end_matches('\0');
        output.push_str(&format!("{name}\t{} bytes\n", info.size));
    }
    Ok(output)
}

fn write_image(image: &Path, name: &str, source: &Path) -> Result<String, String> {
    let metadata = fs::metadata(source).map_err(|e| e.to_string())?;
    if metadata.len() > 64 * 1024 * 1024 {
        return Err("arquivo ultrapassa o limite operacional de 64 MiB por chamada".into());
    }
    let data = fs::read(source).map_err(|e| e.to_string())?;
    if data.len() > 64 * 1024 * 1024 {
        return Err("arquivo ultrapassa o limite operacional de 64 MiB por chamada".into());
    }
    let mut volume = HermioneVolume::mount(ImageDevice::open(image)?)
        .map_err(|e| format!("montagem falhou: {e:?}"))?;
    volume
        .write_file(name, &data)
        .map_err(|e| format!("gravação falhou: {e:?}"))?;
    volume
        .into_device()
        .flush()
        .map_err(|e| format!("sincronização falhou: {e:?}"))?;
    Ok(format!(
        "Arquivo '{name}' gravado e sincronizado no volume."
    ))
}

fn read_image(image: &Path, name: &str, destination: &Path) -> Result<String, String> {
    let mut volume = HermioneVolume::mount(ImageDevice::open(image)?)
        .map_err(|e| format!("montagem falhou: {e:?}"))?;
    let mut files: [Option<FileInfo>; carlio_os::hermione::MAX_FILES] =
        [None; carlio_os::hermione::MAX_FILES];
    volume
        .list(&mut files)
        .map_err(|e| format!("listagem falhou: {e:?}"))?;
    let info = files
        .iter()
        .flatten()
        .find(|entry| {
            core::str::from_utf8(&entry.name)
                .unwrap_or("")
                .trim_end_matches('\0')
                == name
        })
        .ok_or_else(|| "arquivo não encontrado".to_owned())?;
    let mut data = vec![0; info.size as usize];
    let size = volume
        .read_file(name, &mut data)
        .map_err(|e| format!("leitura/verificação falhou: {e:?}"))?;
    data.truncate(size);
    fs::write(destination, data).map_err(|e| e.to_string())?;
    Ok(format!(
        "Arquivo '{name}' validado e salvo em {}.",
        destination.display()
    ))
}

fn scrub_image(path: &Path) -> Result<String, String> {
    let mut volume = HermioneVolume::mount(ImageDevice::open(path)?)
        .map_err(|e| format!("montagem falhou: {e:?}"))?;
    let report: ScrubReport = volume.scrub().map_err(|e| format!("scrub falhou: {e:?}"))?;
    volume
        .into_device()
        .flush()
        .map_err(|e| format!("sincronização falhou: {e:?}"))?;
    Ok(format!(
        "Scrub concluído: {} arquivos verificados; {} cópias reparadas.",
        report.files_verified, report.copies_repaired
    ))
}

fn help() -> &'static str {
    "Administração do filesystem experimental Hermione v3.\n\nUso:\n  herm-fs status\n  herm-fs criar <imagem-nova> <setores>\n  herm-fs listar <imagem>\n  herm-fs gravar <imagem> <nome> <arquivo-origem>\n  herm-fs ler <imagem> <nome> <arquivo-destino>\n  herm-fs scrub <imagem>\n\n'criar' recusa substituir um caminho existente. Use somente arquivos-imagem descartáveis."
}

use carlio_os::FILESYSTEM_NAME;

fn status() -> String {
    format!("{FILESYSTEM_NAME}: BlockDevice, volume v3, journal redo, espelho e scrub disponíveis; montagem exige uma imagem ou dispositivo explícito.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_two_tib_image_accepts_last_lba_and_rejects_past_end() {
        let path = env::temp_dir().join(format!(
            "carlio-herm-fs-2tib-{}-{}.img",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sectors = MAX_VOLUME_SECTORS;
        let bytes = sectors * SECTOR_SIZE as u64;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        file.set_len(bytes).unwrap();
        drop(file);

        let mut image = ImageDevice::open(&path).unwrap();
        assert_eq!(image.sector_count(), sectors);
        let last_lba = sectors - 1;
        let mut sector = [0u8; SECTOR_SIZE];
        sector[0] = 0x5a;
        image.write_sector(last_lba, &sector).unwrap();
        image.flush().unwrap();
        let mut readback = [0u8; SECTOR_SIZE];
        image.read_sector(last_lba, &mut readback).unwrap();
        assert_eq!(readback[0], 0x5a);
        assert_eq!(
            image.read_sector(sectors, &mut readback),
            Err(FsError::Device)
        );
        drop(image);
        fs::remove_file(path).unwrap();
    }
}

#[cfg(test)]
mod existing_tests {
    use super::*;
    #[test]
    fn help_documents_image_commands_and_safe_creation() {
        let text = help();
        assert!(text.contains("criar <imagem-nova>"));
        assert!(text.contains("scrub <imagem>"));
        assert!(text.contains("recusa substituir"));
    }

    #[test]
    fn file_backed_volume_survives_close_and_reopen() {
        let path = env::temp_dir().join(format!("herm-fs-test-{}.img", std::process::id()));
        let source = path.with_extension("source");
        let output = path.with_extension("output");
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&source);
        let _ = fs::remove_file(&output);
        create_image(&path, 128).unwrap();
        fs::write(&source, b"persistente em arquivo-imagem").unwrap();
        write_image(&path, "nota.txt", &source).unwrap();
        read_image(&path, "nota.txt", &output).unwrap();
        assert_eq!(fs::read(&output).unwrap(), b"persistente em arquivo-imagem");
        assert!(scrub_image(&path)
            .unwrap()
            .contains("1 arquivos verificados"));
        assert!(create_image(&path, 128).is_err());
        fs::remove_file(path).unwrap();
        fs::remove_file(source).unwrap();
        fs::remove_file(output).unwrap();
    }

    #[test]
    fn status_describes_block_contract_and_version() {
        let text = status();
        assert!(text.contains(FILESYSTEM_NAME));
        assert!(text.contains("journal redo"));
    }
}

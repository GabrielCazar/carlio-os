//! Administra serviços do sistema através das interfaces CLI de systemd e OpenRC.

use std::{
    env,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

const USAGE: &str = "Uso: carlio-service [--backend auto|systemd|openrc] COMANDO
  backend                         mostra o gerenciador selecionado
  list                            lista serviços conhecidos pelo gerenciador
  status SERVIÇO                  consulta o estado do serviço
  start|stop|restart|reload SERVIÇO
  enable|disable SERVIÇO          altera a inicialização automática
  --help                          mostra esta ajuda";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Backend {
    Systemd,
    OpenRc,
}

impl Backend {
    fn name(self) -> &'static str {
        match self {
            Self::Systemd => "systemd",
            Self::OpenRc => "openrc",
        }
    }

    /// Exige seleção explícita quando os dois ambientes parecem ativos.
    fn detect(systemd: bool, openrc: bool) -> Result<Self, &'static str> {
        match (systemd, openrc) {
            (true, false) => Ok(Self::Systemd),
            (false, true) => Ok(Self::OpenRc),
            (true, true) => Err("systemd e OpenRC detectados. Selecione com --backend."),
            (false, false) => Err("nenhum gerenciador ativo detectado. Selecione com --backend."),
        }
    }

    /// Retorna apenas operações e executáveis nativos fixos. O nome validado
    /// segue como argumento separado, inclusive quando indica uma instância systemd.
    fn invocation(self, action: &str, service: Option<&str>) -> (&'static str, Vec<String>) {
        match (self, service) {
            (Self::Systemd, None) => (
                "systemctl",
                strings(&[
                    "--system",
                    "--no-pager",
                    "list-unit-files",
                    "--type=service",
                ]),
            ),
            (Self::Systemd, Some(name)) => {
                let mut args = strings(&["--system", "--no-pager", action, "--"]);
                args.push(if name.ends_with(".service") {
                    name.to_owned()
                } else {
                    format!("{name}.service")
                });
                ("systemctl", args)
            }
            (Self::OpenRc, None) => ("rc-status", strings(&["--all"])),
            (Self::OpenRc, Some(name)) => match action {
                "enable" => ("rc-update", strings(&["add", name, "default"])),
                "disable" => ("rc-update", strings(&["del", name, "default"])),
                operation => ("rc-service", strings(&[name, operation])),
            },
        }
    }
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

/// Limita a operação a um nome literal. Exclui opções, caminhos e globbing.
fn valid_service(name: &str) -> bool {
    name.len() <= 247
        && name
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'@' | b':')
        })
}

#[derive(Debug, Eq, PartialEq)]
enum Action<'a> {
    Help,
    Backend,
    List,
    Service(&'a str, &'a str),
}

fn parse<'a>(args: &[&'a str]) -> Result<(Option<Backend>, Action<'a>), &'static str> {
    let (backend, args) = if let ["--backend", value, tail @ ..] = args {
        let backend = match *value {
            "auto" => None,
            "systemd" => Some(Backend::Systemd),
            "openrc" => Some(Backend::OpenRc),
            _ => return Err("backend inválido"),
        };
        (backend, tail)
    } else {
        (None, args)
    };
    let action = match args {
        ["--help" | "-h"] => Action::Help,
        ["backend"] => Action::Backend,
        ["list"] => Action::List,
        [action @ ("status" | "start" | "stop" | "restart" | "reload" | "enable" | "disable"), name] =>
        {
            if !valid_service(name) {
                return Err("nome de serviço inválido");
            }
            Action::Service(action, name)
        }
        _ => return Err("comando ou argumentos inválidos"),
    };
    Ok((backend, action))
}

/// Procura somente nos diretórios de ferramentas administrativas do sistema.
fn executable(name: &str) -> Option<PathBuf> {
    ["/usr/bin", "/usr/sbin", "/bin", "/sbin"]
        .iter()
        .map(|dir| Path::new(dir).join(name))
        .find(|path| {
            path.metadata()
                .is_ok_and(|info| info.is_file() && info.permissions().mode() & 0o111 != 0)
        })
}

fn main() -> ExitCode {
    let args: Option<Vec<_>> = env::args_os()
        .skip(1)
        .map(|arg| arg.into_string().ok())
        .collect();
    let Some(args) = args else {
        eprintln!("carlio-service: argumentos devem usar UTF-8");
        return ExitCode::from(2);
    };
    let args: Vec<_> = args.iter().map(String::as_str).collect();
    let (requested, action) = match parse(&args) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("carlio-service: {error}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    if action == Action::Help {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let backend = match requested.map_or_else(
        || {
            Backend::detect(
                Path::new("/run/systemd/system").is_dir(),
                Path::new("/run/openrc/softlevel").is_file(),
            )
        },
        Ok,
    ) {
        Ok(backend) => backend,
        Err(error) => {
            eprintln!("carlio-service: {error}");
            return ExitCode::FAILURE;
        }
    };
    let (program, args) = match action {
        Action::Backend => {
            println!("{}", backend.name());
            return ExitCode::SUCCESS;
        }
        Action::List => backend.invocation("list", None),
        Action::Service(action, name) => backend.invocation(action, Some(name)),
        Action::Help => unreachable!("help handled before backend selection"),
    };
    let Some(path) = executable(program) else {
        eprintln!("carlio-service: executável nativo {program} ausente");
        return ExitCode::from(127);
    };
    // Substituir o processo preserva sinais, stdin e o código de saída do backend.
    let error = Command::new(path).args(args).exec();
    eprintln!("carlio-service: falha ao executar {program}: {error}");
    ExitCode::from(126)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_options_paths_patterns_and_extra_arguments() {
        for name in ["--now", "../sshd", "sshd*", "sshd\n", "", "svc?"] {
            assert!(parse(&["start", name]).is_err(), "{name:?}");
        }
        assert!(parse(&["start", "sshd", "other"]).is_err());
        assert!(parse(&["--backend", "unknown", "list"]).is_err());
        assert_eq!(
            parse(&["--backend", "openrc", "reload", "sshd"]),
            Ok((Some(Backend::OpenRc), Action::Service("reload", "sshd")))
        );
    }

    #[test]
    fn maps_native_operations_and_keeps_enablement_separate() {
        assert_eq!(
            Backend::Systemd.invocation("status", Some("worker@blue")),
            (
                "systemctl",
                strings(&[
                    "--system",
                    "--no-pager",
                    "status",
                    "--",
                    "worker@blue.service"
                ])
            )
        );
        assert_eq!(
            Backend::OpenRc.invocation("enable", Some("sshd")),
            ("rc-update", strings(&["add", "sshd", "default"]))
        );
        assert_eq!(
            Backend::OpenRc.invocation("disable", Some("sshd")),
            ("rc-update", strings(&["del", "sshd", "default"]))
        );
        assert_eq!(
            Backend::OpenRc.invocation("restart", Some("sshd")),
            ("rc-service", strings(&["sshd", "restart"]))
        );
    }

    #[test]
    fn detects_only_unambiguous_runtime_markers() {
        assert_eq!(Backend::detect(true, false), Ok(Backend::Systemd));
        assert_eq!(Backend::detect(false, true), Ok(Backend::OpenRc));
        assert!(Backend::detect(false, false).is_err());
        assert!(Backend::detect(true, true).is_err());
    }
}

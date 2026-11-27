# Gerenciador de serviços CarlIO

O `carlio-service` é uma ferramenta em Rust para administradores de hosts Linux.
Use a mesma interface em máquinas com systemd ou OpenRC. O backend executa
as operações, ordena dependências e aplica sua política de autorização.

```sh
cargo build --locked --release --bin carlio-service
./target/release/carlio-service backend
./target/release/carlio-service list
./target/release/carlio-service status sshd
sudo ./target/release/carlio-service restart sshd
sudo ./target/release/carlio-service --backend openrc enable sshd
```

Use o nome de serviço da distribuição. O modo automático identifica os
marcadores `/run/systemd/system` e `/run/openrc/softlevel`. Em chroot,
contêiner ou ambiente com marcadores ambíguos, use `--backend systemd` ou
`--backend openrc`. O comando `backend` mostra a seleção. `status` consulta o
serviço e preserva o código de saída nativo para automação.

| Comando | systemd | OpenRC |
| --- | --- | --- |
| `list` | Unit files de tipo service | Serviços dos runlevels |
| `status` | `systemctl status` | `rc-service SERVIÇO status` |
| `start`, `stop`, `restart`, `reload` | Operação sobre a unidade `.service` de sistema | Operação definida pelo script OpenRC |
| `enable`, `disable` | Configuração de ativação da unidade | Associação ao runlevel `default` |

A inicialização automática e o estado em execução são operações independentes.
`reload` depende do suporte no serviço. A saída mantém o formato nativo.

O binário aceita nomes literais e executa as ferramentas dos diretórios
administrativos do sistema. Ele chama o backend sem shell, encaminha o
terminal e opera com as permissões do invocador. Instale-o como executável
comum. Exponha operações privilegiadas somente pela política administrativa
do host. Os backends exigem as ferramentas da distribuição e um ambiente
compatível. A supervisão de processos é responsabilidade do backend.

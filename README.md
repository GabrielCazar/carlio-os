# CarlioOS

Sistema operacional experimental, escrito em Rust e inicialmente direcionado a
x86_64. O desenvolvimento usa Arch Linux; o primeiro alvo executável é QEMU e,
depois, servidores físicos x86_64.

O idioma padrão do sistema é **português do Brasil (pt_BR)**: menu de boot,
mensagens do núcleo, terminal e interface. Comandos usam palavras sem acento
para facilitar a entrada em terminais seriais; as mensagens são UTF-8.
O splash serial e o comando `sobre` apresentam “carlIO OS | Criado por João G.R.Santos”.

## Fases

| Fase | Entrega | Estado |
| --- | --- | --- |
| 1. Inicialização | Boot x86_64, kernel base, PCI/AHCI, terminal e testes em QEMU | Concluída — escopo boot e AHCI experimental QEMU |
| 2. Armazenamento protegido | Volume desbloqueado no boot, Hermione inspirado em ZFS, integridade/scrub/reparo redundante e SQLite | Em início — protótipo host e teste AHCI QEMU; sem volume persistente |
| 3. Segurança e contas | Ring 3, memória isolada, processos, syscalls e permissões efetivas | Planejada |
| 4. Aplicações Linux | Formatos AppImage, Snap, Flatpak, DEB, RPM e Arch; runtime/ABI em sandbox | Planejada |
| 5. Windows Mode | VM Windows isolada, integração AD DS e validação de Duo Desktop/Falcon no convidado | Planejada |
| 6. Interface e plataformas | Vídeo, teclado, mouse, imagens, perfis de hardware e avaliação ARM64 | Planejada |

Roadmap e critérios: [`docs/ROADMAP.md`](docs/ROADMAP.md). Há um relatório
Markdown para cada fase em [`reports/`](reports/). Painel: [`progress/index.html`](progress/index.html).
Hardware de referência local e do NAS: [`docs/HARDWARE-BASE.md`](docs/HARDWARE-BASE.md).
Camadas e contratos de segurança: [`docs/ARQUITETURA-KERNEL.md`](docs/ARQUITETURA-KERNEL.md).
O projeto do filesystem Hermione está em [`docs/HERMIONE.md`](docs/HERMIONE.md).
Seu utilitário de linha de comando é `herm-fs`. O formato será inspirado em
ZFS, mas não interoperável com pools ZFS existentes.

## Segurança e limites do protótipo

O terminal atual executa em ring 0 e não fornece isolamento. A política de
permissões é apenas demonstrativa. Existe um driver AHCI experimental limitado
ao QEMU e a testes em imagem descartável. Ainda não existem volume criptografado,
filesystem persistente, SQLite ou processos ring 3. Não use este protótipo para
armazenar dados reais.

O armazenamento persistente deverá ser desbloqueado antes da montagem, com
criptografia autenticada e chave apenas em memória. SQLite ficará acima do
filesystem e não guardará chaves. O bootloader permanece um componente externo
e a imagem de boot também precisará de uma cadeia de confiança.

## Construção e validação

No Arch Linux, dependências: `rust`, `rustup`, `qemu-system-x86`, `limine` e
`xorriso`. Para o kernel bare-metal é necessário o toolchain nightly com
`rust-src` e o alvo `x86_64-unknown-none`; os testes host rodam com stable.

```sh
cd carlio-os
cargo test --all-targets
cargo run --bin herm-fs -- --help
cargo run --bin herm-fs -- status
RUSTUP_HOME=/tmp/carlio-rustup CARGO_HOME=/tmp/carlio-cargo rustup toolchain install nightly --component rust-src,rustfmt --target x86_64-unknown-none
RUSTUP_HOME=/tmp/carlio-rustup CARGO_HOME=/tmp/carlio-cargo rustup run nightly cargo build --release --bin carlio-kernel --target x86_64-unknown-none
./scripts/build-iso.sh
qemu-system-x86_64 -m 512M -cdrom build/carlio-os.iso -boot d -serial stdio -display none
./scripts/test-nas.sh
```

Comandos do terminal, em pt_BR: `ajuda`, `sobre`, `usuario`, `permissoes`,
`limpar`, `parar`. Toda alteração que afete comportamento deve passar pelos
testes locais aplicáveis e por `scripts/test-nas.sh`; o script valida BIOS e
UEFI no NAS/KVM. Resultados ficam em
`reports/logs/`.

## Compatibilidade

Formatos de pacotes Linux não são, por si, compatibilidade de aplicações.
Executar seus programas requer ABI, syscalls, bibliotecas e sandbox. O Windows
Mode será uma VM Windows licenciada; não promete executar agentes proprietários
com Wine. Isso permite testar os agentes no sistema convidado para o qual foram
feitos.

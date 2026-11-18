# Userland e estrutura modular

## Layout lógico do sistema

O VFS adotará FHS como hierarquia canônica para compatibilidade com Arch,
Debian e RPM. Os diretórios CarlioOS planejados serão aliases/views opcionais,
sem mudar os caminhos de instalação dos pacotes. A árvore detalhada, os
diretórios de estado de cada gerenciador e os critérios de montagem estão na
seção [layout lógico do VFS compatível com pacotes](#layout-lógico-do-vfs-compatível-com-pacotes).

## Permissões estilo Linux (política de VFS)

`src/vfs.rs` modela bits POSIX `rwx` owner/group/other, UID, GID primário e
grupos suplementares. A criação aplica `umask`; diretórios setgid transmitem
GID e o bit setgid para subdiretórios. A busca de caminhos exige `x` em cada
diretório, inclusive em `.` e `..`. Root contorna leitura/escrita, enquanto
execução de arquivo ainda exige um bit `x`. `chown` é restrito a root e `chmod`
ao dono/root. O catálogo de programas usa os grupos da sessão antes de chamar
o loader.

O scheduler agora guarda UID/GID, grupos suplementares e `umask` no contexto de
cada processo; o banco host de contas persiste hashes opacos como arquivo
Hermione e autentica por um verificador injetável. A ABI Linux já entra por
`SYSCALL` e o probe executa `getpid`/`exit`. Ainda não há KDF integrado, login de
boot, aplicação dessas credenciais em syscalls VFS gerais, ACLs nem ownership
persistido nos filesystems. FAT32 não armazena permissões POSIX; será necessário aplicar
opções de mount ou metadados laterais, enquanto ext4 pode fornecer metadados
nativos quando seu driver for integrado.

## Módulos Rust

- `src/vfs.rs`: diretórios, caminhos, metadados e política POSIX/Linux host.
- `src/userland/login.rs`: prompt e sessão produzida por um backend de credenciais.
- `src/userland/apps.rs`: catálogo e autorização antes do despacho ao carregador.
- `src/memory.rs`: traduções HHDM e alocador de frames independente do heap.

`src/process.rs`, `src/elf.rs`, `src/linux_abi.rs` e `src/user_memory.rs` já
contêm suporte parcial: ELF ET_EXEC/PIE sem relocations, argv/env, ownership de
páginas, CPL3, entrada `SYSCALL`, cópias validadas e probes de identidade,
`write`, rejeição de `read` inválido e `openat/read/close` positivo em arquivo
VFS inline. Falta execução geral de ELF, VMA/mmap/brk, leitura serial positiva
CPL3 automatizada, scheduler SMP CPL3 validado, sinais, threads e ABI necessária
a libc.

O preparador ELF também aceita `PT_INTERP`: mapeia programa e interpretador
ET_DYN no mesmo address space, entrega o controle ao entry do interpretador e
monta `AT_PHDR`, `AT_PHENT`, `AT_PHNUM`, `AT_PAGESZ`, `AT_BASE` e `AT_ENTRY`.
O interpretador, e não o kernel, fica responsável por dependências e
relocations dinâmicas. Esse caminho ainda não é chamado por um syscall
`execve`: não existe substituição transacional do processo em execução. O VFS
do kernel limita arquivos inline a 16 KiB e ainda não oferece acesso geral a
arquivos grandes/montados e symlinks; portanto não carrega o `ld-linux.so` real
da userland Arch neste estado. O teste host usa executáveis sintéticos pequenos.

## Mounts e interoperabilidade

O VFS agora expõe `create_fhs_layout()` para criar os caminhos-base de uma
userland Linux e uma tabela limitada de mounts com source, tipo de filesystem,
opções `ro`, `noexec` e `nosuid`, unmount privilegiado e seleção do mount mais
específico para um caminho. A validação exige diretórios existentes e caminhos
absolutos canônicos. Os tipos reconhecidos incluem Hermione, FAT32, ext4, proc
e dev.

O VFS já delega leituras ao backend fornecido pelo chamador: Hermione, FAT32 e
um subconjunto ext4. A instância do kernel ainda não descobre partições nem
monta volumes no boot. Syscalls de arquivo acessam apenas arquivos inline do
VFS, com descritores limitados por PID (`open/openat/read/write/lseek/close`);
não alcançam ainda os backends mounted. Permanecem pendentes escrita POSIX nos
mounts, metadados por inode e execução de opções mount. A ABI não inicia pacman
ou binários Arch. `.PKGINFO` ainda é apenas inspecionado; extração e instalação
transacional de pacotes não foram implementadas.

O kernel também tem tabela bounded de sockets IPv4/TCP/UDP por processo e liga
as syscalls Linux `socket`, `bind`, `listen` e `close`; `bind` valida
`sockaddr_in`, conflitos de endereço/porta e privilégio para portas abaixo de
1024. Isso reserva portas e modela sockets, sem `accept`, `connect`, `send`,
`recv` ou filas de dados de sockets.

Para a NIC virtual QEMU Intel 82574L (`8086:10d3`), o driver polling DMA fornece
quadros Ethernet TX/RX e a inicialização faz DHCPv4 DORA. O endereço obtido é
registrado apenas no diagnóstico; ainda não alimenta uma tabela de rotas nem um
resolvedor DNS. Acesso geral à internet, TCP/IP conectado a sockets e drivers
físicos Intel I219-LM e Realtek RTL8111 permanecem pendentes.

## Integrações de desktop e IA

## Bases Arch Linux e Ubuntu sobre o kernel CarlioOS

O objetivo é manter o kernel CarlioOS e executar acima dele userlands Arch
Linux e Ubuntu. Isso não equivale a inicializar os kernels Arch ou Ubuntu: os
binários oficiais Arch são ELF Linux e esperam ABI, syscalls, formato de
processos e pseudo-filesystems Linux. Para iniciar a base de forma útil, o
compat layer terá de suportar primeiro ELF dinâmico x86_64, processos e
syscalls necessários ao init, `/proc`, `/sys`, `/dev`, sinais, IPC, permissões
Linux e chamadas de sistema usadas por glibc e systemd.

Arch e Ubuntu são bases de userland previstas para o mesmo kernel CarlioOS e
para a ABI Linux compartilhada. O suporte a pacotes mantém gerenciadores e
formatos separados: pacman/libalpm e repositórios Arch; APT e pacotes `.deb`
para Ubuntu. Leitores host atuais interpretam `.PKGINFO` e campos `control`
(incluindo arquitetura `amd64`), sem extrair ou instalar arquivos. A instalação
de pacotes continuará separada da execução: scripts, binários e serviços só
serão habilitados após sandbox, assinatura e rollback.

A entrada `SYSCALL` do kernel exercita em CPL3 `getpid`/`exit`,
`getuid`/`geteuid`/`getgid`/`getegid`, `getgroups`, `write` para serial e a
rejeição de um destino inválido em `read`. `write` copia de uma página de
usuário somente leitura após validar o intervalo; `read(0)` valida o intervalo
gravável completo antes de consumir serial e devolve linhas por `fd 0`. O probe
também abre `/programas/probe`, lê um byte de arquivo inline no VFS e fecha o
descritor. Leitura positiva de entrada serial CPL3 ainda não está automatizada;
os descritores ainda não alcançam backends mounted. Permanecem pendentes VMA
real para `mmap/munmap/brk`, libc, execução geral de ELF, linker/relocations,
pseudo-filesystems e carregamento de rootfs. Arch e Ubuntu
compartilham o dispatcher Linux, mas seguem metas de compatibilidade, não bases
executáveis entregues.

O smoke de segurança roda uma fixture com UID/GID 1000: `openat` concede acesso
ao arquivo permitido e nega com `EACCES` um arquivo root-owned modo `0600`.
BIOS/UEFI local e NAS/KVM passaram em 16/11/2026. Isso comprova somente essa
rota no dispatcher do fixture; login, processos gerais e mounts não estão
integrados.

### Layout lógico do VFS compatível com pacotes

O VFS apresentará FHS como hierarquia lógica canônica. O particionamento e os
mounts físicos serão decisão separada. Pacotes Arch, Debian e RPM verão os
caminhos esperados sem reescrever arquivos ou ensinar um layout CarlioOS
privado aos instaladores.

```text
/
├── bin -> usr/bin          # compatibilidade de caminhos históricos
├── boot                   # kernel, initramfs e configuração do boot
├── dev                    # nós de dispositivo e interfaces de device manager
├── etc                    # configuração local, inclusive de pacotes
├── home                   # diretórios dos usuários
├── lib -> usr/lib         # compatibilidade de caminhos históricos
├── lib64 -> usr/lib       # compatibilidade x86_64 quando requerida
├── media                  # mídia removível montada pelo sistema
├── mnt                    # mounts administrativos temporários
├── opt                    # aplicações de terceiros com layout próprio
├── proc                   # pseudo-filesystem de processos/kernel
├── root                   # home do administrador
├── run                    # estado volátil de serviços e sockets
├── sbin -> usr/bin        # compatibilidade de caminhos históricos
├── snap                   # pontos de montagem de aplicações Snap
├── srv                    # dados servidos por serviços (ex.: arquivos)
├── sys                    # pseudo-filesystem de dispositivos/kernel
├── tmp                    # temporários efêmeros
├── usr/
│   ├── bin, sbin, lib, lib32, include, share
│   ├── local/             # software instalado pelo administrador
│   └── lib/<triplet>/     # bibliotecas x86_64 por ABI
└── var/
    ├── cache/             # caches de pacotes e aplicações
    ├── lib/               # estado persistente de serviços e gerenciadores
    ├── log/               # logs persistentes
    ├── spool/              # filas de serviços
    └── tmp/                # temporários que sobrevivem a limpeza de /tmp
```

As ligações `/bin`, `/sbin`, `/lib` e `/lib64` são aliases de compatibilidade
para a hierarquia unificada em `/usr`; elas serão expostas somente quando o VFS
suportar symlinks corretamente.

| Caminho canônico | Uso | Categoria planejada CarlioOS |
| --- | --- | --- |
| `/usr/bin`, `/usr/sbin`, `/usr/lib`, `/usr/share` | programas, bibliotecas e dados distribuídos | `/programas` (visão somente) |
| `/etc` | configuração local do sistema | `/configuracoes` (visão somente) |
| `/var` | estado mutável, logs, caches e bancos de pacotes | `/sistema` (atalho administrativo opcional para estado CarlioOS) |
| `/var/lib/pacman`, `/var/lib/dpkg`, `/var/lib/rpm` | bancos/metadados de pacotes por formato | subárvores privadas de cada gerenciador |
| `/var/lib/flatpak`, `/var/lib/snapd`, `/snap` | estado e aplicações Flatpak/Snap | subárvores próprias; montagem conforme o runtime |
| `/home` e `/root` | dados e configuração dos usuários | `/usuarios` (alias opcional de `/home`) |
| `/run` | estado volátil de serviços desde o boot | gestão de serviços |
| `/tmp` e `/var/tmp` | temporários efêmeros e persistentes | `/temporarios` (alias opcional de `/tmp`) |
| `/dev`, `/proc`, `/sys` | dispositivos e pseudo-filesystems do kernel | interfaces do kernel |
| `/boot`, `/opt`, `/srv`, `/mnt`, `/media` | boot, software adicional, serviços e mounts | mantidos segundo as convenções FHS |

Os nomes planejados `/programas`, `/configuracoes`, `/usuarios`, `/sistema`,
`/dados` e `/temporarios` serão aliases/views opcionais: `/programas` → `/usr`,
`/configuracoes` → `/etc`, `/usuarios` → `/home`, `/sistema` → estado próprio
em `/var/lib/carlio`, `/dados` → `/srv` e `/temporarios` → `/tmp`. Nunca serão
prefixos de instalação de pacotes nem substituirão os caminhos FHS. Os bancos
ficam em `/var/lib/pacman`, `/var/lib/dpkg`, `/var/lib/rpm`,
`/var/lib/flatpak` e `/var/lib/snapd`; caches correspondentes ficam sob
`/var/cache`. Para reduzir riscos, aliases/views só serão criados após suporte
do VFS a symlinks/mounts e testes com caminhos absolutos dos pacotes. O layout
ainda não está montado no kernel atual.

### Console nativo

O console interativo da userland será o zsh fornecido pelo Arch Linux quando a
ABI necessária estiver disponível. Como alternativa, poderá ser criado um shell
em Rust inspirado na experiência zsh; compatibilidade de sintaxe, comandos e
plugins será especificada e validada por plugin, sem presumir que plugins
arbitrários funcionem. O prompt serial atual é somente diagnóstico do kernel e
não substitui esse console de usuário.

Plasma e SDDM são componentes de userland Linux e dependem de DBus, PAM, sessão
gráfica, compositor, serviços e ABI do kernel. O kernel atual não oferece esses
contratos. ChatGPT e Ollama serão provedores opcionais atrás de uma interface
de agentes; rede, TLS, gestão segura de credenciais e processo de userland são
pré-requisitos. Nenhuma credencial será compilada no kernel.

### Bootstrap inicial da userland Arch

O perfil e o construtor host ficam em [`userland/arch/`](../userland/arch/).
`build-rootfs.sh` monta um subconjunto oficial x86_64 a partir de uma data
explícita do Arch Linux Archive e grava a saída sob
`/tmp/carlioos-arch-userland-*`. O perfil exclui systemd, kernel Linux, AUR e
desktop; o bootstrap usa `pacstrap -N` e não tem fallback privilegiado. Cada
saída registra a revisão e o estado do checkout do kernel. A preparação no host
não é instalação no CarlioOS: boot, ABI dinâmica e execução da rootfs continuam
pendentes.

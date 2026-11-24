# Relatório — Fase 4: aplicações e pacotes Linux

### Correção — ABI VFS, PT_INTERP e bootstrap de userlands — 2026-11-24

O painel ainda apontava lacunas de validação em `stat`, mutações e `fsync`,
e tratava o interpretador ELF como ausente. O limite de leitura de ELF em
`src/main.rs` também reutilizava os 16 KiB do VFS inline. O limite do
executável e do interpretador montados passou para 512 KiB por arquivo,
mantendo o limite de páginas por processo. A tentativa de usar heap para
esses buffers expandiu o heap e alterou o baseline de frames do teste de
processos. A versão validada reutiliza os buffers estáticos do loader.

- [x] Um ELF de 32 KiB vindo de ext4 testa `mkdir`, `openat`, `write`,
  `fchmod`, `fsync`, `fstat`, `newfstatat`, `close`, `unlink` e `rmdir`.
  Verifica tamanho e modo retornados e os erros `EFAULT`, `EBADF` e
  `ENOENT`. A imagem usa overlay em RAM. [BIOS local](logs/fase5-abi/local-bios.log),
  [UEFI local](logs/fase5-abi/local-uefi.log), [BIOS NAS/KVM](logs/fase5-abi/nas-bios.log)
  e [UEFI NAS/KVM](logs/fase5-abi/nas-uefi.log).
- [x] Na variante PIE, um interpretador de teste de 256 KiB lê `AT_BASE`
  e `AT_ENTRY`, transfere controle ao ELF principal e exige que o teste
  VFS complete. [BIOS local](logs/fase5-abi/local-interp-bios.log),
  [UEFI local](logs/fase5-abi/local-interp-uefi.log), [BIOS NAS/KVM](logs/fase5-abi/nas-interp-bios.log)
  e [UEFI NAS/KVM](logs/fase5-abi/nas-interp-uefi.log).
- [x] Arch de 2026/11/23 com 110 pacotes e zsh 5.9.2, executado em chroot
  sob o Linux do host. A preparação usou um checkout isolado de `origin/main`
  e cache de pacotes de uma tentativa anterior, com assinaturas de pacotes
  obrigatórias. A instalação puxou systemd como dependência transitiva e
  relatou falha em um hook de initcpio. O construtor verifica os pacotes
  instalados e publica o manifesto. A verificação elimina o falso sucesso
  observado quando `pacstrap -N` retornou zero após timeout de download.
  [Bootstrap](logs/fase5-abi/arch-bootstrap.log) e
  [retorno zero sem instalação recusado](logs/fase5-abi/arch-bootstrap-falha.log).
- [x] Ubuntu Base 24.04.5 amd64 verificada por assinatura do manifesto e
  SHA-256, extraída no host e testada com `dpkg-query` e `/usr/bin/true`
  em chroot no Linux do host. O manifesto modificado foi recusado.
  [Bootstrap](logs/fase5-abi/ubuntu-bootstrap.log) e [provas host](logs/fase5-abi/ubuntu-host.log).

No teste exploratório em BIOS local, `execve` carregou o `true` do Ubuntu e
chegou ao `ld-linux`. A tarefa foi encerrada com `#UD` em CPL3 no endereço
`0x7000000201e3` (`movq %rcx,%xmm1`), com `CR4.OSFXSR` desativado. A
execução de SSE exige habilitação e preservação do estado por tarefa no
scheduler. Esse teste identifica a primeira falha observada na glibc.
[Serial](logs/fase5-abi/ubuntu-carlio-exploratorio-bios.log) e
[registradores e desassemblagem](logs/fase5-abi/ubuntu-carlio-fault.txt).

Reprodução das provas CPL3:

```sh
CARLIO_DISK_EXEC_TEST=1 scripts/build-iso.sh
scripts/test-disk-exec-qemu.sh build/carlio-os.iso /tmp/carlioos-vfs-bios.log
CARLIO_TEST_EXEC_INTERP=1 scripts/test-disk-exec-qemu.sh build/carlio-os.iso /tmp/carlioos-interp-bios.log
```

Passar `--firmware /usr/share/edk2/x64/OVMF_CODE.4m.fd` para UEFI e
`--accel kvm` no NAS. As imagens de disco são descartáveis. Hardware
físico segue pendente. A execução de glibc e da base das distribuições
no CarlioOS segue aberta. O zsh do snapshot tem 971.816 bytes, acima da
janela atual de leitura de ELF, e o dispatcher ainda usa apenas `mmap`
anônimo. Instalação e rollback de pacotes no alvo continuam pendentes.

## ABI Linux: IDs de thread, relógio monotônico e `brk` CPL3 — 2026-11-19

O dispatcher Linux agora separa `getpid` (PID do grupo) de `gettid` (ID da
thread), fornece `clock_gettime`/`clock_getres` para `CLOCK_MONOTONIC` com
`timespec` x86_64 de 16 bytes e retorna `EINVAL` para clocks ainda não
suportados. A fonte monotônica é o PIT do kernel a 100 Hz; conversões de faixa
usam validação checked. A cópia para user memory passa pelo backend validado.
O backend `ProcessVm` já conectado a `mmap`/`munmap` agora também teve `brk`
provado em CPL3: crescimento, escrita no último byte e redução com liberação da
página. O fixture ELF foi ampliado para acomodar o bytecode adicional.

Provas negativas host cobrem clock não suportado e ponteiro inválido; a ABI
host totaliza 184 testes e Clippy passou. `cargo fmt --check` e
`cargo check --target x86_64-unknown-none --bin carlio-kernel` passaram
([formato](logs/fase4-linux-abi-timers-final-fmt-20261119.txt),
[host](logs/fase4-linux-abi-timers-final-host-20261119.txt),
[Clippy](logs/fase4-linux-abi-timers-final-clippy-20261119.txt),
[kernel](logs/fase4-linux-abi-timers-final-kernel-20261119.txt)). A ISO BIOS e
UEFI local validou `gettid`, relógio monotônico e `mmap/munmap/brk`, mais 27 ACKs
de shootdown ([BIOS](logs/fase4-linux-abi-timers-qemu-bios-20261119-021750.log),
[UEFI](logs/fase4-linux-abi-timers-qemu-uefi-20261119-021750.log)). O NAS/KVM
de 5 GiB passou o gate BIOS, mas UEFI não fechou o smoke completo: a probe de
duas CR3 teve zero trocas do timer (`preempted=0`) e o teste de page tables
recusou declarar sucesso. O mesmo log mostra os novos probes ABI passando; a
falha fica registrada, sem repetir a execução NAS
([resumo](logs/fase4-linux-abi-timers-nas-20261119-021821.txt),
[BIOS](logs/fase4-linux-abi-timers-nas-20261119-021821-qemu-kvm-bios.log),
[UEFI](logs/fase4-linux-abi-timers-nas-20261119-021821-qemu-kvm-uefi.log)).

Uma execução BIOS inicial falhou por fixture ELF de 512 bytes pequena demais;
foi expandida para 1 KiB e a execução local BIOS/UEFI seguinte passou
([falha registrada](logs/fase4-linux-abi-timers-qemu-bios-20261119-021705.log)).
O gate da ABI fica parcialmente validado, não suportado para userlands gerais.
Permanecem fora desta fatia `CLOCK_REALTIME`, `execve` integrado, linker e
dependências ELF dinâmicas, sinais, futex, chamadas de thread completas,
mapeamento de arquivo, mounts no VFS e compatibilidade com glibc/systemd.

## Linker dinâmico, libc e arquivos maiores (2026-11-16)

### Incremento de metadados, símbolos versionados e libc (2026-11-16)

O parser ELF ganhou `ElfDynamicMetadata`, que valida e extrai `PT_INTERP` e
nomes `DT_NEEDED` via `DT_STRTAB`/`DT_STRSZ`, com limites de quantidade e tamanho,
terminação NUL, caminho absoluto do interpretador e rejeição de nomes de
dependência com `/`. O módulo do linker oferece lookup bounded por nome/versão
GNU (nome e hash), respeita símbolos fracos e ignora versões hidden em lookup
sem versão; também fornece fila de dependências deduplicada.

A libc freestanding agora inclui rotinas básicas de `string.h`, `putchar`,
`puts`, `printf` e `snprintf` com conversões inteiras/string/caractere, além dos
wrappers de syscalls anteriores. `malloc`, stdio bufferizado/FILE, locale,
pthread, TLS e ABI completa de glibc continuam fora do suporte.

Este incremento ainda é infraestrutura host, não execução dinâmica: o caminho
de `exec` não carrega o `PT_INTERP`, não mapeia a fila de `.so`, não consome
`DT_SYMTAB`/`DT_VERSYM`/`DT_VERNEED`/`DT_VERDEF` para lookup real nem aplica
relocations contra o conjunto de objetos. A libc também não é suficiente para
zsh ou binários Arch. Os testes cobrem extração de `PT_INTERP`, `DT_NEEDED`,
rejeição de caminho inseguro e lookup/ordem de símbolos; o build `cc` do archive
verifica que a libc compila, sem alegar validação de execução. Regressão final:
154 testes da biblioteca, 1 de `carlio-logs` e 4 de Hermione passaram; a ISO
iniciou em BIOS/UEFI no QEMU local e BIOS/UEFI no NAS/KVM. Logs: [host local](logs/elf-metadata-symbols-libc-20261116-final-host-20261116-134308.txt),
[BIOS local](logs/elf-metadata-symbols-libc-20261116-final-qemu-bios-20261116-134308.log),
[UEFI local](logs/elf-metadata-symbols-libc-20261116-final-qemu-uefi-20261116-134308.log),
[host NAS](logs/elf-metadata-symbols-libc-20261116-final-nas-20261116-134339.txt),
[BIOS NAS/KVM](logs/elf-metadata-symbols-libc-20261116-final-nas-20261116-134339-qemu-kvm-bios.log) e
[UEFI NAS/KVM](logs/elf-metadata-symbols-libc-20261116-final-nas-20261116-134339-qemu-kvm-uefi.log).

O VFS passou a aceitar arquivos de até 16 KiB por nó, inclusive I/O por faixa,
e snapshots `CARLVFS2`. A restauração ainda aceita snapshots `CARLVFS1` já
persistidos. O snapshot Hermione cresce com o limite por arquivo e continua
limitado pela capacidade do volume; não há alocação de arquivos sem limite.

Foi adicionado `dynamic_linker::apply_rela`, que aplica `R_X86_64_RELATIVE`,
`R_X86_64_64`, `R_X86_64_GLOB_DAT` e `R_X86_64_JUMP_SLOT` em segmentos graváveis,
com callback de resolução de símbolos e validação de overflow/faixa. A libc
inicial em `userland/libc` fornece wrappers x86_64 para read/write/open/openat,
close, mmap/munmap, getpid e exit, além de errno e cabeçalhos básicos. O archive
`libcarlio.a` compila com o compilador C do host.

Essa etapa ainda não fecha execução dinâmica de programas: o kernel não carrega
`PT_INTERP`, não percorre `DT_NEEDED`, não carrega `.so`, não implementa lookup
de símbolos/versionamento/hash, TLS, IFUNC ou RELR. A libc não oferece ainda
stdio, malloc, pthreads, resolução de sinais nem ABI suficiente para zsh. O
carregador ELF existente continua útil para executáveis estáticos; o motor de
relocations é uma peça inicial para um linker integrado, não prova de binários
dinâmicos Arch.

Validação desta revisão: 150 testes da biblioteca, 1 teste de `carlio-logs` e 4
testes Hermione passaram em host local e NAS; `userland/libc` gerou
`libcarlio.a` com `cc`; a ISO iniciou em BIOS/UEFI no QEMU TCG local e BIOS/UEFI
no NAS/KVM, incluindo probes CPL3 já existentes. Logs: [host local](logs/dynamic-linker-libc-large-files-20261116-r3-host-20261116-133341.txt),
[BIOS local](logs/dynamic-linker-libc-large-files-20261116-r3-qemu-bios-20261116-133341.log),
[UEFI local](logs/dynamic-linker-libc-large-files-20261116-r3-qemu-uefi-20261116-133341.log),
[host NAS](logs/dynamic-linker-libc-large-files-20261116-nas-20261116-133414.txt),
[BIOS NAS/KVM](logs/dynamic-linker-libc-large-files-20261116-nas-20261116-133414-qemu-kvm-bios.log) e
[UEFI NAS/KVM](logs/dynamic-linker-libc-large-files-20261116-nas-20261116-133414-qemu-kvm-uefi.log).

## Matrizes de compatibilidade de pacotes (2026-11-16)

Foi criada [`docs/MATRIZ-PACOTES.md`](../docs/MATRIZ-PACOTES.md) com estados
separados para inspeção, autenticidade, dependências, instalação/remoção,
execução, isolamento e atualização/rollback de ALPM, DEB, RPM, Flatpak, Snap e
AppImage. O estado atual reconhecido como parcial continua limitado à leitura
host de `.PKGINFO` e `control`; nenhum formato tem fluxo de instalação ou
execução de payload validado. O documento também registra pré-requisitos e a
ordem dos gates, sem declarar funcionalidades planejadas como implementadas.

## ABI, PIE e pseudo-filesystems — primeira fatia (2026-11-15)

Naquela primeira fatia, o dispatcher expunha contratos backend para `close(3)`, `lseek(8)`,
`mmap(9)`, `munmap(11)` e `brk(12)`, além das chamadas anteriores. As chamadas
de memória são delegadas a um backend que deve impor validação de VMA. A entrada
`SYSCALL` e a fatia CPL3 de `getpid`/`exit` foram integradas posteriormente,
como detalhado adiante; VMA e semânticas de produção seguem pendentes.

O parser ELF aceita PIE `ET_DYN` quando o kernel fornece um load bias e valida
overflow/faixa de usuário ao aplicar o bias a entry e segmentos. O loader agora
aplica relocations locais `R_X86_64_RELATIVE` de `RELA` em static PIE; não
resolve `DT_NEEDED`, símbolos, PLT nem carrega o interpretador dinâmico. Isso é
fundação de relocations, não um linker dinâmico completo.

`pseudo_fs` oferece snapshots somente leitura por offset para um conjunto
inicial de `/proc` e `/sys` (`uptime`, `loadavg`, `meminfo`, `cpuinfo`, versão,
CPU online). Os valores vêm de `SystemSnapshot`; ainda não há montagem no VFS
nem coleta de métricas reais do kernel.

Validação inicial: 108 testes host passaram localmente e no NAS; logs dessa
revisão permanecem arquivados em `abi-pie-pseudofs-*`.

## Integração inicial no kernel e leitores de pacotes (2026-11-15)

O kernel configura STAR/LSTAR/FMASK/EFER e pilha por CPU para a instrução
`SYSCALL`. A validação inicial de `getpid`/`exit` descrita nesta seção foi
ampliada em 16/11: o probe CPL3 também exercita getters de credenciais/grupos,
`write` serial de página de usuário somente leitura e rejeição de `read` com
destino inválido. `read(0)` agora consome linhas da serial depois de validar o
buffer completo; a leitura positiva em CPL3 não fazia parte daquela validação
histórica e foi acrescentada em 16/11, como registrado adiante.

Foram adicionados leitores bounded host para `.PKGINFO` Arch e `control`
Debian/Ubuntu (`amd64`/`all`), com validação de campos/dependências. Não há
extração de `.pkg.tar.zst`/`.deb`, resolução de dependências, instalação nem
scripts de pacote. A ISO para NAS fica para depois de completar a base Arch.

Validação após ligar o opcode `SYSCALL`: 118 testes host local/NAS e boots
BIOS/UEFI local/NAS-KVM passaram, com `ABI Linux x86_64: getpid/exit via
SYSCALL passou.` Logs: [host local](logs/arch-ubuntu-syscall-final-host-20261115-205558.txt),
[host NAS](logs/arch-ubuntu-syscall-final-nas-20261115-205620.txt),
[BIOS local](logs/arch-ubuntu-syscall-final-qemu-bios-20261115-205558.log),
[UEFI local](logs/arch-ubuntu-syscall-final-qemu-uefi-20261115-205558.log),
[BIOS NAS](logs/arch-ubuntu-syscall-final-nas-20261115-205620-qemu-kvm-bios.log) e
[UEFI NAS](logs/arch-ubuntu-syscall-final-nas-20261115-205620-qemu-kvm-uefi.log).

Arch e Ubuntu são bases de userland paralelas sobre a mesma ABI Linux do kernel.
Os parsers `.PKGINFO` e `control` cobrem metadados iniciais, não rootfs nem
instalação. A prova CPL3 de `getpid`/`exit` valida apenas a porta comum
`SYSCALL`, não executa binários oficiais de nenhuma das distribuições. A ISO
para NAS permanece condicionada à conclusão e validação da base Arch; Ubuntu
também ainda não dispõe de rootfs executável.

## Identidade POSIX exposta na ABI (2026-11-16)

Dispatcher e kernel encaminham a identidade do processo para
`getuid`/`geteuid`/`getgid`/`getegid`; `getgroups` copia grupos suplementares
para memória de usuário validada. Testes host cobrem identidade não-root e
grupos. A prova CPL3 posterior verifica o retorno dos cinco getters. Validação
histórica: 120 testes host, build `no_std` e boots BIOS/UEFI locais e NAS/KVM.
Consulte os logs
[`fase3-5-credenciais-syscall-*`](logs/fase3-5-credenciais-syscall-nas-20261116-015111.txt)
neste diretório.

## Incremento do dispatcher Linux x86_64 (2026-11-15)

O dispatcher host agora reconhece `read(0)` além de `write(1)`, `getpid(39)` e
`exit`/`exit_group`. A operação usa buffer kernel limitado, trata EOF, retorno
parcial, descritor inválido, erro de cópia para user e número desconhecido
(`-ENOSYS`). `LinuxSyscallIo` fornece a cópia validada ao espaço user e o acesso
ao descritor; o dispatcher não desreferencia ponteiros user diretamente.

O teste host valida leitura, cópia para user e `-EFAULT`. A entrada kernel agora
implementa `read(0)` sobre serial e valida o buffer gravável integral antes de
ler; não há descritores de arquivos conectados ao VFS nem compatibilidade com
libc/pacman comprovada.

Validação: os 101 testes host passaram localmente e no NAS; boot de regressão
BIOS/UEFI passou no QEMU local e NAS/KVM. Evidências: [host local](logs/fase3-5-incrementos-host-20261115-194208.txt),
[host NAS](logs/fase3-5-incrementos-nas-20261115-194229.txt), [BIOS local](logs/fase3-5-incrementos-qemu-bios-20261115-194208.log),
[UEFI local](logs/fase3-5-incrementos-qemu-uefi-20261115-194208.log), [BIOS NAS](logs/fase3-5-incrementos-nas-20261115-194229-qemu-kvm-bios.log)
e [UEFI NAS](logs/fase3-5-incrementos-nas-20261115-194229-qemu-kvm-uefi.log).
Após os novos testes VFS, a revisão completa passou 102 testes host local e NAS
e manteve os quatro boots BIOS/UEFI aprovados; logs host:
[local](logs/fase3-5-fechamento-host-20261115-194459.txt) e
[NAS](logs/fase3-5-fechamento-nas-20261115-194519.txt).

## Meta consolidada do produto (2026-11-15)

Objetivo: kernel CarlioOS em Rust, userland Arch Linux e compatibilidade
progressiva com `.deb`, `.rpm`, `.exe`/PE, AppImage, Flatpak/Flathub e Snap.
Compatibilidade será acompanhada separadamente para inspeção, instalação,
dependências, execução, sandbox e atualização. Flathub é tratado como origem de
aplicações Flatpak. Executar `.exe` requer compatibilidade Windows ou convidado
virtualizado; suporte ao pacote não implica execução.

O console de usuário será zsh da userland Arch quando a ABI permitir, ou um shell
em Rust inspirado no zsh com compatibilidade de plugins testada individualmente.
O shell serial atual continua sendo ferramenta de diagnóstico do kernel.

**Progresso desta etapa:** há um dispatcher host-testável para syscalls x86_64
Linux básicas (`write`, `getpid`, `exit`/`exit_group`) com retorno `-ENOSYS`
para as demais. Ainda não está ligado à entrada da instrução `SYSCALL`, ao
console serial, ao VFS ou ao scheduler, então não executa binários Linux.

**Estado:** fundação inicial em andamento. O VFS host aplica a seleção POSIX
owner/group/other e o catálogo consulta permissões antes do loader. O kernel
tem provas controladas CPL3 de syscalls básicas; não executa processos gerais
nem oferece uma ABI Linux suficiente para iniciar uma distribuição.

## Prioridade atual — dispatcher inicial (2026-11-16)

O dispatcher Linux compartilhado por Arch e Ubuntu tem provas CPL3 para
`getpid/exit`, credenciais/grupos, `write` serial, rejeição de destino inválido
em `read`, leitura positiva de `read(0)` serial e `openat/read/close` positivo
para um arquivo do VFS inline. Os descritores de arquivo não chegam aos
backends mounted. `mmap/munmap` anônimo privado agora aloca folhas próprias no
CR3 por processo, e `brk` usa VMA e page tables do mesmo backend; ainda faltam
probe CPL3 dedicado de `brk` e mmap de arquivo/compartilhado. Leitores host continuam limitados a
`.PKGINFO` e `control`; linker/relocations, rootfs, extração/instalação de
pacotes, libc e validação de binários reais seguem pendentes.

Host: 140 testes aprovados. Build `no_std`, probes BIOS/UEFI local e NAS/KVM e
smoke de armazenamento com mídia descartável passaram. Logs: [`host`](logs/syscall-vfs-fd-final-host-20261116-120515.txt),
[`BIOS local`](logs/syscall-vfs-fd-final-qemu-bios-20261116-120515.log),
[`UEFI local`](logs/syscall-vfs-fd-final-qemu-uefi-20261116-120515.log),
[`NAS/KVM`](logs/syscall-vfs-fd-final-nas-20261116-120551.txt),
[`filesystem NAS/KVM`](logs/syscall-vfs-fd-final-nas-20261116-120551-filesystem-qemu-kvm.log).

## Probe positivo de entrada serial CPL3 (2026-11-16)

O probe Linux pede um byte em `read(0)` para buffer gravável no espaço do
processo. `scripts/qemu-smoke.py` espera o aviso do kernel e envia `Q` pela
serial; o resultado só passa quando o dispatcher retorna um byte. Isso cobre
entrada serial positiva em ring 3 no cenário automatizado, além do caso
negativo de ponteiro inválido. Não transforma o polling serial atual em driver
de entrada assíncrona nem adiciona `poll/select`.

Validação: 140 testes host, build da ISO e boots BIOS/UEFI no QEMU TCG e
BIOS/UEFI no NAS/KVM passaram. Logs: [host local](logs/kernel-hermione-completion-host-20261116-121449.txt),
[BIOS local](logs/kernel-hermione-completion-qemu-bios-20261116-121449.log),
[UEFI local](logs/kernel-hermione-completion-qemu-uefi-20261116-121449.log),
[host NAS](logs/kernel-context-switch-nas-20261116-121512.txt),
[BIOS NAS/KVM](logs/kernel-context-switch-nas-20261116-121512-qemu-kvm-bios.log) e
[UEFI NAS/KVM](logs/kernel-context-switch-nas-20261116-121512-qemu-kvm-uefi.log).

## Backend de memória Linux por processo (2026-11-16)

Cada processo da prova CPL3 agora possui `ProcessVm`: tabela VMA fixa,
`OwnedAddressSpace` e integração com `LinuxSyscallIo`. `mmap` implementa
`MAP_PRIVATE | MAP_ANONYMOUS` com páginas zeroed e rollback; `munmap` atualiza
as folhas e as regiões; `brk` usa o mesmo estado, verifica colisões e mapeia ou
libera páginas. Mapeamentos fixos, compartilhados e de arquivos retornam erro
explícito. O reaper destrói o espaço e suas folhas. A prova ring 3 abriu uma
página anônima, gravou nela e a desmapeou com sucesso.

Capacidade corrente é fixa (128 folhas e 64 VMAs por processo na prova), sem
page fault lazy e sem crescimento da tabela. O estado de processos ainda está
limitado aos dois slots especializados da prova de boot; isso não é runtime de
execução geral de processos.

Validação final: 142 testes host locais e NAS; build ISO e BIOS/UEFI no QEMU
TCG e BIOS/UEFI no NAS/KVM. Logs: [host local](logs/per-process-vm-backend-final-host-20261116-124123.txt),
[BIOS local](logs/per-process-vm-backend-final-qemu-bios-20261116-124123.log),
[UEFI local](logs/per-process-vm-backend-final-qemu-uefi-20261116-124123.log),
[host NAS](logs/kernel-context-switch-nas-20261116-124144.txt),
[BIOS NAS/KVM](logs/kernel-context-switch-nas-20261116-124144-qemu-kvm-bios.log) e
[UEFI NAS/KVM](logs/kernel-context-switch-nas-20261116-124144-qemu-kvm-uefi.log).

## Escopo

Prioridade: ecossistema Arch Linux, com repositórios pacman/libalpm, formato
`.pkg.tar.*`, metadados ALPM, dependências e integração futura com systemd.
O kernel CarlioOS permanece próprio; a meta é executar a userland Arch acima
dele por uma ABI Linux compatível, sem inicializar o kernel Arch. Desenvolver o
gerenciador em Rust e separar inspeção/instalação da execução;
scripts de instalação não rodam como administrador por padrão. Em seguida,
tratar AppImage, Snap, Flatpak, DEB e RPM como formatos distintos. Execução de
programas depende de ABI Linux e sandbox, não apenas do reconhecimento do pacote.

## Gerenciador de serviços planejado

O gerenciador de serviços da userland compatível com Linux será o **systemd**.
Essa etapa depende da ABI Linux necessária, incluindo os serviços e interfaces
de kernel exigidos pelo systemd, além do modelo de processos e isolamento da
fase 3. A escolha é arquitetural; systemd ainda não está portado nem executável
no CarlioOS.

## Critérios de aceite

Matriz pública de suporte por formato/versão; integridade e origem verificadas;
rollback testado; suíte de aplicações representativa executa isolada e testes
negativos provam que não acessa recursos fora das permissões.

## Estado comprovado

Não há parser de pacotes, runtime Linux, ABI, syscalls ou sandbox. Nenhum
formato (incluindo Arch) está implementado ainda.

Uma etapa preparatória implementou UID/GID, grupos suplementares, modos POSIX,
`umask`, herança setgid, busca por diretórios e regras básicas de root no
VFS host. A sessão de login pode carregar grupos suplementares e o catálogo
verifica `x` antes do despacho. Isso não permite executar nenhum binário: o
loader atual é apenas uma interface e ainda faltam processos gerais, espaços
isolados executáveis, ELF loader e syscalls de produção. Consulte
[relatório de segurança e permissões](fase-3-seguranca.md)
e [contrato da userland](../docs/USERLAND.md).

## Atualização ABI Arch/Plasma (2026-11-16)

O dispatcher compartilhado implementa `uname(2)` no layout Linux x86_64
(`sysname`, `nodename`, `release`, `version`, `machine`, `domainname`, cada
campo com 65 bytes), copiando o resultado somente para memória de usuário
validada pelo backend. Os valores identificam o contrato Linux x86_64 do
CarlioOS sem expor dados da máquina host. Testes host cobrem conteúdo, offsets
e ponteiro inválido.

Este incremento não fecha a ABI para Arch nem torna Plasma instalável: ainda
faltam linker dinâmico/relocations, `execve` geral, rootfs persistente, extração
e transações ALPM, libc e validação de binários oficiais. Plasma/SDDM depende
também de threads/`clone`/`futex`, sinais, relógio, DBus, entrada e driver de
vídeo/compositor. Esses gates seguem abertos no roadmap.

Validação deste incremento: 143 testes da biblioteca, 1 teste de `carlio-logs`
e 4 de `herm-fs` passaram localmente e no NAS. ISO BIOS/UEFI passou em QEMU TCG
local e KVM no NAS. Logs: [host local](logs/arch-plasma-abi-uname-host-20261116-124558.txt),
[BIOS local](logs/arch-plasma-abi-uname-qemu-bios-20261116-124558.log),
[UEFI local](logs/arch-plasma-abi-uname-qemu-uefi-20261116-124558.log),
[host NAS](logs/arch-plasma-abi-uname-nas-20261116-124640.txt),
[BIOS NAS/KVM](logs/arch-plasma-abi-uname-nas-20261116-124640-qemu-kvm-bios.log)
e [UEFI NAS/KVM](logs/arch-plasma-abi-uname-nas-20261116-124640-qemu-kvm-uefi.log).

### Static PIE: relocations locais `RELA` (2026-11-16)

O loader aceita `PT_DYNAMIC` sem `PT_INTERP` e aplica entradas
`R_X86_64_RELATIVE` sem símbolo: valida tags `DT_RELA*`, limites da tabela,
alvos inteiramente dentro de segmentos graváveis e overflow do bias/addend. A
cópia das relocations ocorre antes das permissões ELF finais, sem abrir janela
RWX. Dependências (`DT_NEEDED`), relocations por símbolo/PLT, interpretador
dinâmico e libc continuam recusados; binários Arch reais ainda não são
executados.

Validação deste passo: 144 testes da biblioteca, 1 de `carlio-logs` e 4 de
`herm-fs` passaram localmente e no NAS. ISO BIOS/UEFI passou em QEMU TCG local
e KVM no NAS. Logs: [host local](logs/arch-static-pie-rela-final-host-20261116-125510.txt),
[BIOS local](logs/arch-static-pie-rela-final-qemu-bios-20261116-125510.log),
[UEFI local](logs/arch-static-pie-rela-final-qemu-uefi-20261116-125510.log),
[host NAS](logs/arch-static-pie-rela-final-nas-20261116-125614.txt),
[BIOS NAS/KVM](logs/arch-static-pie-rela-final-nas-20261116-125614-qemu-kvm-bios.log)
e [UEFI NAS/KVM](logs/arch-static-pie-rela-final-nas-20261116-125614-qemu-kvm-uefi.log).

### Planejamento de dependências ALPM (2026-11-16)

Adicionado `userland::alpm::resolve`: recebe catálogo de metadados Arch já
parseados e raízes solicitadas, resolve dependências nomeadas com restrições
`>=`, `<=`, `>`, `<` e `=`, deduplica e devolve ordem topológica com dependências
antes dos dependentes. A busca é limitada a 64 pacotes e detecta pacote ausente,
versão incompatível, metadado inválido e ciclos. A comparação cobre segmentos
alfanuméricos/númericos comuns; não implementa toda a semântica `vercmp`, epoch,
`provides` ou conflitos do libalpm.

O Hermione já persiste arquivos em dispositivo de bloco com journal redo,
espelho e recuperação na remontagem, mas o kernel ainda não o monta como rootfs
gravável para syscalls Linux. Este incremento não baixa repositórios, verifica
assinaturas, extrai `.pkg.tar.zst`, aplica transações/rollback ou executa hooks.
Não há libc/linker dinâmico nem execução validada de binários Arch/Ubuntu reais;
static PIE com relocation local não substitui essas peças.

Validação: 146 testes da biblioteca, 1 `carlio-logs` e 4 `herm-fs`; QEMU BIOS/
UEFI TCG local e QEMU/KVM BIOS/UEFI no NAS aprovados. Logs: [host
local](logs/arch-alpm-planner-20261116-host-20261116-125944.txt), [BIOS
local](logs/arch-alpm-planner-20261116-qemu-bios-20261116-125944.log), [UEFI
local](logs/arch-alpm-planner-20261116-qemu-uefi-20261116-125944.log), [host
NAS](logs/arch-alpm-planner-20261116-nas-20261116-130016.txt), [BIOS
NAS](logs/arch-alpm-planner-20261116-nas-20261116-130016-qemu-kvm-bios.log) e
[UEFI NAS](logs/arch-alpm-planner-20261116-nas-20261116-130016-qemu-kvm-uefi.log).

### Hermione como rootfs Linux (2026-11-16)

O kernel agora tenta montar o dispositivo AHCI como Hermione, sem formatá-lo.
Só aceita um superbloco Hermione válido. A árvore VFS é persistida no arquivo
`rootfs.img` do volume em um snapshot versionado com CRC; no primeiro boot de
um volume Hermione vazio, cria a hierarquia FHS e o snapshot inicial. Snapshot
corrompido ou erro de leitura não é sobrescrito. Syscalls de arquivo continuam
passando pelas verificações de credenciais do VFS; `openat` com `O_CREAT` e
`O_TRUNC`, `write` e `read`/`lseek` agora veem a árvore restaurada, e alterações
de criação/truncamento/escrita são gravadas no Hermione.

O teste host cobre VFS aninhado -> snapshot Hermione -> desmontagem/remontagem
-> leitura do mesmo caminho. QEMU TCG com imagem Hermione AHCI descartável
confirmou a inicialização do snapshot e a remontagem no segundo boot. O teste
de regressão no NAS não tinha dispositivo AHCI utilizável e usou VFS em RAM sem
formatar disco. O rootfs continua limitado a 32 nós e arquivos inline de 512
bytes; falha de gravação retorna `EIO` mas ainda não reverte o estado já
alterado em RAM. Não há `mkdir`, symlinks, `execve` geral, mapeamento ELF de
arquivos grandes, base Arch instalável, libc ou zsh executável. O shell zsh e a
base Arch seguem bloqueados por essas peças e pelo extrator/transacionador ALPM.

Validação host: 148 testes da biblioteca, 1 `carlio-logs` e 4 `herm-fs`, local e
NAS. QEMU BIOS/UEFI TCG local e KVM BIOS/UEFI no NAS passaram como regressão de
boot. O teste positivo rootfs AHCI em dois boots QEMU TCG está registrado em
[inicialização](logs/herm-rootfs-final-disk-initialize-qemu-bios-20261116.log)
e [remontagem](logs/herm-rootfs-final-disk-remount-qemu-bios-20261116.log).
Logs da suíte: [host local](logs/herm-rootfs-final-20261116-host-20261116-132256.txt),
[BIOS local](logs/herm-rootfs-final-20261116-qemu-bios-20261116-132256.log),
[UEFI local](logs/herm-rootfs-final-20261116-qemu-uefi-20261116-132256.log),
[host NAS](logs/herm-rootfs-final-20261116-nas-20261116-132345.txt), [BIOS
NAS](logs/herm-rootfs-final-20261116-nas-20261116-132345-qemu-kvm-bios.log) e
[UEFI NAS](logs/herm-rootfs-final-20261116-nas-20261116-132345-qemu-kvm-uefi.log).

## Bootstrap host da base Arch — início (2026-11-16)

Foi adicionado um perfil enxuto de pacotes Arch x86_64 e um script host para
preparar uma rootfs usando um snapshot explícito do Arch Linux Archive. O
construtor limita saídas a `/tmp/carlioos-arch-userland-*`, exige `pacstrap -N`
sem fallback privilegiado e registra no artefato a revisão do kernel, o estado
do checkout, o snapshot e a lista de pacotes. A revisão remota consultada no
início do trabalho era `ccc30642a6e9ee2de06fb14edc71af0659c36477`; builds futuros
registram as revisões observadas no momento da execução.

O perfil inclui `filesystem`, `glibc`, `bash`, `coreutils`, `zsh`, `pacman`,
`archlinux-keyring` e `tzdata`; exclui systemd, kernel Linux, AUR e desktop. Isso
é apenas preparação host de uma árvore de arquivos, não instalação ou execução
no kernel CarlioOS. O ABI dinâmico, montagem no boot, pacman funcional no alvo,
isolamento e rollback continuam pendentes.

Checagens: `bash -n userland/arch/build-rootfs.sh` e `git diff --check` passaram.
`pacstrap` não está instalado neste host, então a geração da rootfs não foi
executada. Nenhum fonte do kernel foi alterado; QEMU/NAS e testes de kernel não
foram executados nesta fatia. Log: [checagens host](logs/phase5-arch-userland-bootstrap-20261116-host.txt).

Próximo gate: executar o construtor em host Arch preparado com
`arch-install-scripts`, snapshot válido e namespaces de usuário habilitados;
depois registrar inventário/manifesto da rootfs e avançar a integração apenas
quando loader dinâmico, glibc e montagem persistente estiverem disponíveis.

## Resumo consolidado do progresso — 2026-11-16

O pedido de concluir a userland exige três integrações ainda abertas; os
incrementos existentes são fundações, não suporte funcional completo:

| Frente | Estado comprovado/documentado | Gate restante |
|---|---|---|
| Loader dinâmico ELF | Parser limitado de `PT_INTERP`/`DT_NEEDED`, execução ELF estática/PIE limitada e resolução de símbolos em biblioteca host | Integrar o interpretador e dependências `.so` ao `exec`, carregar tabelas GNU de versões e aplicar relocations necessárias; validar executáveis reais em CPL3 |
| ABI da libc/glibc | Dispatcher `SYSCALL` parcial, probes CPL3 selecionados e `libcarlio.a` freestanding | Implementar e validar semânticas/syscalls requeridas pela glibc, inclusive heap/VMA, mmap de arquivo, TLS/threads e sinais conforme os binários-alvo |
| Descoberta e mounts | AHCI experimental, snapshot Hermione inicializado e remontado em QEMU; backends de leitura host e tabela VFS | Descobrir tabela de partições, identificar filesystem, montar no boot e rotear operações VFS/syscalls aos backends; provar positivo, negativo e falha |

Rootfs Arch instalável, glibc, zsh e binários Arch continuam sem execução
validada no kernel. Não foram executados novos testes nesta atualização
documental. A página [`progress/index.html`](../progress/index.html)
e o [roadmap](../docs/ROADMAP.md) foram alinhados com esses limites; os gates de
kernel permanecem abertos e exigem evidências host, QEMU BIOS/UEFI e NAS/KVM
aplicáveis em `reports/logs/` antes de qualquer estado de suporte.

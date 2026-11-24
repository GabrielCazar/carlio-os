# Fase 4 — mounts, rede e namespace interoperável

## Permissões do VFS montado no kernel

O VFS exige o bit de busca em cada diretório do backend montado antes de
consultar metadados, ler ou executar um arquivo. A autorização usa UID, GID
e grupos suplementares. A leitura também exige permissão no inode alvo.
FAT32 expõe metadados sintéticos para a raiz e seus subdiretórios.

A prova ext4, executada como UID 1000, exige `EACCES` em `access`,
`openat` e `execve` para um arquivo `0755` dentro de um diretório
`0700` de UID 0. Na mesma execução, o ELF permitido testa metadados,
mutações e `fsync` no overlay. O host também cobre a concessão por
grupo suplementar em diretório `0710`.

- Ext4 em QEMU TCG local: [BIOS](logs/fase4-vfs-local/ext4-bios.log)
  e [UEFI com PT_INTERP](logs/fase4-vfs-local/ext4-uefi.log).
- FAT32 em QEMU TCG local: montagem e leitura de `/probe.bin` em
  [BIOS](logs/fase4-vfs-local/fat32-bios.log) e
  [UEFI](logs/fase4-vfs-local/fat32-uefi.log). A imagem reutiliza o
  gerador FAT32 do teste de persistência porque o host estava sem `mcopy`.
- Overlay persistente em QEMU TCG local: BIOS e UEFI recuperaram dados
  e modo após reinicialização e depois de corromper o checksum do cabeçalho
  mais recente. [BIOS](logs/fase4-vfs-local/overlay-bios.log) e
  [UEFI](logs/fase4-vfs-local/overlay-uefi.log).
- Host: 200 testes aprovados. Formatação do pacote e Clippy passaram.

Para reproduzir a prova ext4, compile a ISO com `CARLIO_DISK_EXEC_TEST=1`
e execute `scripts/test-disk-exec-qemu.sh ISO LOG`. Para a variante UEFI,
use `CARLIO_TEST_EXEC_INTERP=1` e passe `--firmware` com o OVMF.
A prova de persistência usa uma ISO compilada com `CARLIO_STORAGE_TEST=1`
e `scripts/test-persistence-qemu.py --iso ISO --vfs --log-dir DIRETORIO`.

A política validada cobre a fatia de VFS implementada no kernel.
As montagens ext4/FAT32 permanecem em leitura e as mutações usam o overlay.
Login, contas persistentes ligadas ao kernel e serviços de rede continuam
como gates do bloco 4.

## Histórico de novembro de 2026

### Entrega inicial

- `Vfs::create_fhs_layout()` cria caminhos FHS básicos para userlands Linux.
- `Vfs::mount()` registra source, tipo de filesystem e opções `ro`, `noexec` e
  `nosuid` em diretório existente, somente para UID 0.
- `Vfs::mount_for()` seleciona o mount mais específico por caminho e respeita
  fronteiras entre componentes (`/mnt` não cobre `/mnt2`).
- `Vfs::unmount()` remove registro de mount, também exigindo UID 0.
- `Vfs::read_mounted_file()` delega ao backend tipado associado ao mount mais
  específico; Hermione, FAT32 e ext4 implementam o contrato somente leitura.
- FAT32 percorre diretórios por nomes curtos 8.3 e ext4 lê superbloco, tabela
  de grupos, inodes, diretórios lineares e extents.
- AHCI agora fornece adaptador `BlockDevice` serializado e o teste Hermione de
  QEMU usa esse adaptador; a descoberta ainda mantém somente o primeiro disco.
- Testes cobrem caminhos inválidos/inexistentes, mount duplicado, usuário sem
  privilégio, seleção aninhada e diretórios FHS.
- Driver polling DMA para Intel 82574L/QEMU (`8086:10d3`) faz TX/RX e DHCPv4
  DORA (DISCOVER/OFFER/REQUEST/ACK); smoke QEMU recebe endereço IPv4.
- ABI Linux liga `socket`, `bind`, `listen` e `close` à tabela bounded de
  sockets IPv4 TCP/UDP por PID, com conflitos de portas e portas privilegiadas.

### Limites da entrega inicial

A delegação exige que o chamador forneça o backend/dispositivo correto; o boot
normal ainda não descobre partições nem conecta o AHCI ao VFS. FAT32 só lê nomes
8.3. O leitor ext4 é uma fatia somente leitura: não interpreta journal, htree,
symlinks, checksum de metadados, arquivos esparsos nem recursos incompatíveis.
Não há escrita FAT32/ext4 nem rootfs Hermione. `read_mounted_file` valida
permissão no diretório do mount, mas ainda não aplica metadados POSIX do inode.
`noexec` e `nosuid` não estão ligados a execução; `ro` não governa escrita
porque a API de backend ainda é somente leitura. A ABI Linux permanece insuficiente para iniciar
pacman ou uma userland Arch; pacotes continuam apenas com leitura de `.PKGINFO`.
Os sockets reservam portas mas ainda não transferem dados: faltam accept,
connect, send/recv e integração com o TCP/IP. DHCP obtém lease, mas falta
instalar rota, DNS e validar acesso geral à internet. O driver NIC suporta
somente QEMU Intel 82574L; RTL8111 e I219 físicos seguem pendentes.

### Validação da entrega inicial

- Host final: 136 testes aprovados; `cargo fmt --check` e `git diff --check`
  passaram. Log: [`host final`](logs/mounts-ext4-fat32-final-host-20261116-114554.txt).
- Boot da ISO em QEMU TCG local BIOS/UEFI aprovado: [`BIOS`](logs/mounts-ext4-fat32-final-qemu-bios-20261116-114554.log)
  e [`UEFI`](logs/mounts-ext4-fat32-final-qemu-uefi-20261116-114554.log).

- Host: 135 testes aprovados (inclui leitura positiva/negativa ext4 sintético,
  roteamento VFS tipado e regressão FAT32); log
  [`host`](logs/mount-ext4-fat32-host-20261116-113617.txt).
- QEMU BIOS/UEFI local e NAS/KVM BIOS/UEFI: boots aprovados, sem teste de mount
  conectado ao driver dentro do kernel. Logs: [`BIOS local`](logs/mount-ext4-fat32-qemu-bios-20261116-113617.log),
  [`UEFI local`](logs/mount-ext4-fat32-qemu-uefi-20261116-113617.log),
  [`host NAS`](logs/mount-ext4-fat32-nas-20261116-113650.txt),
  [`BIOS NAS`](logs/mount-ext4-fat32-nas-20261116-113650-qemu-kvm-bios.log) e
  [`UEFI NAS`](logs/mount-ext4-fat32-nas-20261116-113650-qemu-kvm-uefi.log).
- Regressão após a ponte AHCI: 135 testes host e BIOS/UEFI local e NAS/KVM
  aprovados; teste de IDENTIFY, leitura/escrita, erro ATA, Hermione remontado e
  scrub passaram somente na imagem raw descartável. Logs: [`host local`](logs/mounts-ahci-final-host-20261116-114210.txt),
  [`BIOS local`](logs/mounts-ahci-final-qemu-bios-20261116-114210.log),
  [`UEFI local`](logs/mounts-ahci-final-qemu-uefi-20261116-114210.log),
  [`AHCI local`](logs/mounts-ahci-final-filesystem-qemu.log),
  [`host NAS`](logs/mounts-ahci-final-nas-20261116-114336.txt),
  [`BIOS NAS`](logs/mounts-ahci-final-nas-20261116-114336-qemu-kvm-bios.log),
  [`UEFI NAS`](logs/mounts-ahci-final-nas-20261116-114336-qemu-kvm-uefi.log) e
  [`AHCI NAS`](logs/mounts-ahci-final-nas-20261116-114336-filesystem-qemu-kvm.log).

- Host: `cargo test --all-targets`, 128 testes aprovados; log
  `logs/fase-4-rede-final-host-20261116-073336.txt`.
- QEMU local BIOS e UEFI: boots e DHCPv4 DORA aprovados; logs
  `logs/fase-4-rede-final-qemu-bios-20261116-073336.log` e
  `logs/fase-4-rede-final-qemu-uefi-20261116-073336.log`.
- NAS/KVM BIOS e UEFI: 128 testes host no NAS, boots e DHCPv4 DORA aprovados;
  logs `logs/fase-4-rede-final-nas-20261116-074655.txt` e
  `logs/fase-4-rede-final-nas-20261116-074655-qemu-kvm-{bios,uefi}.log`.
- ISO regenerada: `build/carlio-os.iso`.
- Revalidação após correção dos scripts e agregador de logs: 129 testes host
  local/NAS; BIOS/UEFI TCG local e KVM NAS; AHCI/Hermione em imagem descartável
  no NAS. Logs: [`host local`](logs/fase-local-host-20261116-110549.txt),
  [`host NAS`](logs/kernel-subsystems-final-20261116-nas-20261116-110614.txt),
  [`BIOS NAS`](logs/kernel-subsystems-final-20261116-nas-20261116-110614-qemu-kvm-bios.log),
  [`UEFI NAS`](logs/kernel-subsystems-final-20261116-nas-20261116-110614-qemu-kvm-uefi.log)
  e [`AHCI NAS`](logs/kernel-subsystems-final-20261116-nas-20261116-110614-filesystem-qemu-kvm.log).
- Regressão geral de 16/11: 128 testes host e QEMU TCG BIOS/UEFI locais; 128
  testes host no NAS e QEMU/KVM BIOS/UEFI. Os boots também obtiveram lease
  DHCPv4. Evidências: [`host local`](logs/fase-local-host-20261116-105456.txt),
  [`BIOS local`](logs/fase-local-qemu-bios-20261116-105456.log),
  [`UEFI local`](logs/fase-local-qemu-uefi-20261116-105456.log),
  [`host NAS`](logs/validacao-20261116-nas-20261116-105219.txt),
  [`BIOS NAS`](logs/validacao-20261116-nas-20261116-105219-qemu-kvm-bios.log)
  e [`UEFI NAS`](logs/validacao-20261116-nas-20261116-105219-qemu-kvm-uefi.log).

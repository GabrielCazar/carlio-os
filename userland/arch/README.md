# Userland Arch Linux

Esta pasta inicia a montagem de uma rootfs Arch Linux x86_64 para o kernel
CarlioOS. O alvo usa os pacotes oficiais do Arch e um snapshot explícito do
Arch Linux Archive. Antes do bootstrap, atualize a referência com
`git fetch origin main`. O construtor registra os SHAs de `HEAD` e
`origin/main`, e recusa o build se a referência remota local não for ancestral
do kernel selecionado.

## Bootstrap inicial

Em um host Arch Linux com `arch-install-scripts`, `pacman` e namespaces de
usuário habilitados, execute como usuário comum:

```sh
./userland/arch/build-rootfs.sh AAAA/MM/DD
```

Substitua `AAAA/MM/DD` por uma data disponível no Arch Linux Archive. A saída é
criada exclusivamente em `/tmp/carlioos-arch-userland-*`; a opção de destino
personalizado só aceita esse mesmo prefixo. O construtor chama `pacstrap -N`
(sem fallback privilegiado), usa um keyring próprio no alvo e exige assinaturas
de pacotes. A configuração, lista de pacotes e revisão/estado do kernel ficam
registrados no diretório da saída.

O perfil inicial em `packages.base.txt` inclui `filesystem`, `glibc`, `bash`,
`coreutils`, `zsh`, `pacman`, `archlinux-keyring` e `tzdata`. Não inclui
systemd, kernel Linux, drivers Linux, AUR nem desktop; o escopo inicial é uma
rootfs mínima para desenvolver e medir a ABI Linux do CarlioOS.

## Estado e limites

Esta ferramenta prepara arquivos de uma rootfs no host; não instala uma rootfs
no CarlioOS. A árvore não é inicializável nem executável pelo kernel atual. A
ABI Linux ainda não tem loader dinâmico integrado, resolução completa de
dependências `.so`, glibc funcional, pseudo-filesystems montados ou execução
geral de ELF. Instalação de pacotes no kernel, autenticação transacional ALPM,
sandbox de payload e rollback continuam pendentes em
`docs/MATRIZ-PACOTES.md`.

O build usa os namespaces user/mount oferecidos por `pacstrap -N`, mas mantém a
conectividade de rede do host para baixar pacotes. Ainda exige revisão de
isolamento antes de usar pacotes não oficiais; AUR e scriptlets fora dos
pacotes do snapshot oficial são explicitamente excluídos.

O dispatcher também oferece implementação inicial de `mprotect(10)` para
intervalos contidos em uma única VMA anônima; a libc freestanding expõe a
declaração e o wrapper. A libc também expõe wrappers iniciais de identidade,
`clock_gettime`/`clock_getres` monotônicos, `brk` e `sbrk`. W+X é negado, `PROT_NONE`
e intervalos fora desse escopo continuam sem suporte. Essas alterações estão
aguardando os gates host/QEMU/NAS e não devem ser consideradas validadas nem
disponíveis em uma rootfs Arch executável.

A libc também declara `getcwd` (cwd fixo `/`), `access`, `fsync`, `fstat`, `stat`, `fstatat` e o layout x86-64 de
`struct stat`; caminhos relativos de `access/openat/fstatat` são interpretados
sob cwd `/`. `fstatat` ignora dirfd em caminhos absolutos e ainda recusa dirfds
relativos diferentes de `AT_FDCWD`; flags não nulas não estão implementadas.
Timestamps permanecem zero e a compatibilidade continua sem validação.
`mkdir`, `umask`, `chmod`, `fchmod`, `chown`, `fchown`, `lchown`, `unlink/unlinkat` e `rmdir` também estão ligados ao VFS inline e ao contexto do processo;
essa extensão aguarda validação.
`fchmodat`, `fchownat` e as formas de `chown` também foram ligadas com as
restrições descritas no registro de implementação local.

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

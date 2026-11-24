#!/usr/bin/env bash
set -euo pipefail

usage() {
    printf 'Uso: %s AAAA/MM/DD [diretorio-de-saida]\n' "${0##*/}" >&2
    printf 'Cria uma rootfs Arch x86_64 sob /tmp/carlioos-arch-userland-* usando snapshot do Arch Linux Archive.\n' >&2
}

fail() {
    printf 'Erro: %s\n' "$*" >&2
    exit 1
}

if [[ $# -lt 1 || $# -gt 2 ]]; then
    usage
    exit 2
fi

snapshot_date=$1
[[ $snapshot_date =~ ^[0-9]{4}/[0-9]{2}/[0-9]{2}$ ]] || fail 'a data do snapshot deve usar AAAA/MM/DD.'
normalized_date=$(date -d "$snapshot_date" +%F 2>/dev/null) || fail 'data de snapshot inválida.'
[[ $normalized_date == "${snapshot_date//\//-}" ]] || fail 'data de snapshot inválida.'
(( EUID != 0 )) || fail 'execute como usuário comum; pacstrap usará namespaces (-N), sem root no host.'

for command in pacstrap pacman git install mkdir mktemp sed date; do
    command -v "$command" >/dev/null 2>&1 || fail "comando obrigatório ausente: $command"
done

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
repo_root=$(cd -- "$script_dir/../.." && pwd -P)
packages_file="$script_dir/packages.base.txt"
[[ -f $packages_file ]] || fail "lista de pacotes ausente: $packages_file"

if [[ $# -eq 2 ]]; then
    output_dir=$2
    [[ $output_dir =~ ^/tmp/carlioos-arch-userland-[A-Za-z0-9._-]+$ ]] \
        || fail 'a saída precisa ser diretório direto sob /tmp com prefixo carlioos-arch-userland-.'
    [[ ! -e $output_dir && ! -L $output_dir ]] || fail 'o diretório de saída já existe.'
    mkdir -m 0755 -- "$output_dir"
else
    output_dir=$(mktemp -d /tmp/carlioos-arch-userland-XXXXXXXX)
fi

rootfs="$output_dir/rootfs"
mkdir -m 0755 -- "$rootfs"

mapfile -t packages < <(sed -e '/^[[:space:]]*#/d' -e '/^[[:space:]]*$/d' "$packages_file")
((${#packages[@]} > 0)) || fail 'a lista de pacotes está vazia.'
for package in "${packages[@]}"; do
    [[ $package =~ ^[[:alnum:]@][[:alnum:]@._+-]*$ ]] || fail "nome de pacote inválido: $package"
done

kernel_revision=$(git -C "$repo_root" rev-parse --verify HEAD)
[[ $kernel_revision =~ ^[0-9a-f]{40}$ ]] || fail 'não foi possível identificar a revisão do kernel.'
upstream_revision=$(git -C "$repo_root" rev-parse --verify refs/remotes/origin/main) \
    || fail 'origin/main não está disponível; atualize as referências remotas antes do bootstrap.'
git -C "$repo_root" merge-base --is-ancestor "$upstream_revision" "$kernel_revision" \
    || fail 'HEAD não contém a revisão mais recente conhecida em origin/main.'
if [[ -n $(git -C "$repo_root" status --porcelain) ]]; then
    kernel_worktree=dirty
else
    kernel_worktree=clean
fi

pacman_conf="$output_dir/pacman.conf"
cat >"$pacman_conf" <<EOF
[options]
Architecture = x86_64
SigLevel = Required DatabaseOptional
LocalFileSigLevel = Required

[core]
Server = https://archive.archlinux.org/repos/$snapshot_date/\$repo/os/\$arch

[extra]
Server = https://archive.archlinux.org/repos/$snapshot_date/\$repo/os/\$arch
EOF

printf 'Construindo rootfs Arch x86_64 para CarlioOS.\n'
printf 'Kernel HEAD: %s (%s; origin/main %s está contido em HEAD).\n' "$kernel_revision" "$kernel_worktree" "$upstream_revision"
printf 'Snapshot Arch: %s. Destino: %s\n' "$snapshot_date" "$rootfs"
printf 'Pacotes: %s\n' "${packages[*]}"

# -N confina a instalação a namespaces user/mount; não há fallback privilegiado.
# -K cria keyring próprio no alvo; assinaturas continuam obrigatórias no pacman.
pacstrap -N -K -C "$pacman_conf" "$rootfs" "${packages[@]}"

# Algumas versões de pacstrap -N retornam zero mesmo após falha do pacman.
# Consulte o banco do alvo antes de publicar o manifesto de sucesso.
package_db="$rootfs/var/lib/pacman"
[[ -d $package_db/local ]] || fail 'banco de pacotes ausente após pacstrap.'
pacman --config "$pacman_conf" --root "$rootfs" --dbpath "$package_db" -Q "${packages[@]}" >/dev/null \
    || fail 'a transação não instalou todos os pacotes do perfil.'
pacman --config "$pacman_conf" --root "$rootfs" --dbpath "$package_db" -Q >"$output_dir/packages.installed.txt"

install -D -m 0644 -- "$pacman_conf" "$rootfs/etc/pacman.conf"
install -D -m 0644 -- "$packages_file" "$output_dir/packages.base.txt"
cat >"$output_dir/build-info.txt" <<EOF
distribution=Arch Linux
architecture=x86_64
arch_archive_snapshot=$snapshot_date
kernel_repository=https://github.com/GabrielCazar/carlio-os
kernel_revision=$kernel_revision
kernel_origin_main=$upstream_revision
kernel_worktree=$kernel_worktree
package_profile=userland/arch/packages.base.txt
rootfs=$rootfs
runtime_status=not-yet-executable-on-carlioos
EOF

printf 'Rootfs preparada em %s\n' "$rootfs"
printf 'Isto não valida boot nem execução de binários Arch no CarlioOS.\n'

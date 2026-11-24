#!/usr/bin/env bash
set -euo pipefail

release=${1:-}
if [[ $# != 1 || ! $release =~ ^[0-9]{2}\.[0-9]{2}\.[0-9]+$ ]]; then
    printf 'Uso: %s AA.MM.PATCH\n' "${0##*/}" >&2
    exit 2
fi
if (( EUID == 0 )); then
    printf 'Execute como usuário comum.\n' >&2
    exit 2
fi

output=$(mktemp -d /tmp/carlioos-ubuntu-userland.XXXXXXXX)
printf 'Saída do bootstrap Ubuntu Base: %s\n' "$output"
cd "$output"
series=${release%.*}
base="https://cdimage.ubuntu.com/ubuntu-base/releases/$series/release"
archive="ubuntu-base-$release-base-amd64.tar.gz"
signing_key=843938DF228D22F7B3742BC0D94AA3F0EFE21092

for file in SHA256SUMS SHA256SUMS.gpg; do
    curl -fsSL --retry 2 --max-time 120 "$base/$file" -o "$file"
done
curl -fsSL --retry 2 --max-time 30 \
    https://archive.ubuntu.com/ubuntu/project/ubuntu-archive-keyring.gpg -o ubuntu.gpg
mkdir -m 700 gnupg
gpg --homedir "$output/gnupg" --no-default-keyring --keyring "$output/ubuntu.gpg" \
    --export "$signing_key" > trusted.gpg
[[ -s trusted.gpg ]]
gpgv --keyring "$output/trusted.gpg" SHA256SUMS.gpg SHA256SUMS

awk -v file="$archive" '
    $2 == file || $2 == "*" file { print; matches++ }
    END { if (matches != 1) exit 1 }
' SHA256SUMS > image.sha256
curl -fsSL --retry 2 --max-time 120 "$base/$archive" -o "$archive"
sha256sum --check --strict image.sha256

mkdir rootfs
tar -xzf "$archive" --no-same-owner -C rootfs
printf 'Ubuntu Base %s amd64 extraída em %s/rootfs.\n' "$release" "$output"

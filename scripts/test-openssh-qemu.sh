#!/usr/bin/env bash
set -euo pipefail

if (( $# < 2 )); then
  echo "Uso: $0 ISO LOG [opções de qemu-smoke.py]" >&2
  exit 2
fi
scripts_dir=$(cd "$(dirname "$0")" && pwd)
root=$(mktemp -d -t carlio-openssh.XXXXXXXX)
trap 'rm -rf -- "$root"' EXIT
sshd=$(command -v sshd)
profile=${CARLIO_OPENSSH_TEST:-version}
case $profile in
  version) expect=version ;;
  config|listen|banner) expect=$profile ;;
  *) echo "CARLIO_OPENSSH_TEST deve ser version, config, listen ou banner." >&2; exit 2 ;;
esac
source "$scripts_dir/fixtures/stage-linux-elf.sh"
install_elf "$root" "$sshd" /usr/bin/sshd

config_mode=0
if [[ $profile != version ]]; then
  config_mode=1
  if [[ $profile == listen || $profile == banner ]]; then
    config_mode=2
  fi
  mkdir -p "$root/etc/ssh" "$root/var/empty" "$root/run/sshd"
  ssh-keygen -q -t ed25519 -N '' -f "$root/etc/ssh/probe_ed25519"
  rm -- "$root/etc/ssh/probe_ed25519.pub"
  cat >"$root/etc/ssh/probe_config" <<'CONFIG'
AddressFamily inet
Port 2222
ListenAddress 0.0.0.0
HostKey /etc/ssh/probe_ed25519
PasswordAuthentication no
PermitRootLogin no
UsePAM no
CONFIG
  if [[ $profile == banner ]]; then
    # Keep the real deadline when tracing timer and signal behavior.
    case ${CARLIO_OPENSSH_BANNER_GRACE:-disabled} in
      default) ;;
      disabled) printf 'LoginGraceTime 0\n' >> "$root/etc/ssh/probe_config" ;;
      *) echo "CARLIO_OPENSSH_BANNER_GRACE deve ser default ou disabled." >&2; exit 2 ;;
    esac
  fi
  printf 'sshd:x:74:74:OpenSSH privilege separation:/var/empty:/usr/bin/nologin\n' >"$root/etc/passwd"
  printf 'sshd:x:74:\n' >"$root/etc/group"
  printf 'passwd: files\ngroup: files\n' >"$root/etc/nsswitch.conf"
  for path in /usr/lib/ssh /usr/libexec /usr/lib/openssh; do
    for helper in sshd-session sshd-auth; do
      if [[ -x $path/$helper ]]; then
        install_elf "$root" "$path/$helper" "$path/$helper"
      fi
    done
  done
fi
as --64 --defsym "CONFIG_MODE=$config_mode" -o "$root/launcher.o" "$scripts_dir/fixtures/openssh-version.s"
ld -static -o "$root/carlio-disk-exec" "$root/launcher.o"
rm -- "$root/launcher.o"
CARLIO_TEST_DISK_APP_ROOT="$root" "$scripts_dir/test-disk-exec-qemu.sh" "$@" --expect-openssh "$expect"

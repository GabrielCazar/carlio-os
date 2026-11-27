#!/usr/bin/env bash
set -euo pipefail

if (( $# < 3 )); then
  echo "Uso: bash $0 smbd|pacman ISO LOG [opções de qemu-smoke.py]" >&2
  exit 2
fi
app=$1
shift
case "$app" in
  smbd|pacman) ;;
  *) echo "Aplicação desconhecida: $app" >&2; exit 2 ;;
esac

scripts_dir=$(cd "$(dirname "$0")" && pwd)
root=$(mktemp -d -t carlio-package.XXXXXXXX)
trap 'rm -rf -- "$root"' EXIT
source "$scripts_dir/fixtures/stage-linux-elf.sh"
install_elf "$root" "$(command -v "$app")" "/usr/bin/$app"

# Execute only the version query. No daemon, package transaction or host data.
as --64 -o "$root/launcher.o" - <<ASM
.global _start
.text
_start:
    xor %edx, %edx
    push %rdx
    lea option(%rip), %rax
    push %rax
    lea executable(%rip), %rdi
    push %rdi
    mov %rsp, %rsi
    mov \$59, %eax
    syscall
    neg %eax
    mov %eax, %edi
    mov \$60, %eax
    syscall
    ud2
.section .rodata
executable: .asciz "/usr/bin/$app"
option: .asciz "--version"
ASM
ld -static -o "$root/carlio-disk-exec" "$root/launcher.o"
rm -- "$root/launcher.o"
CARLIO_TEST_DISK_APP_ROOT="$root" "$scripts_dir/test-disk-exec-qemu.sh" \
  "$@" --expect-package-version "$app"

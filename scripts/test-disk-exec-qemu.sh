#!/usr/bin/env bash
set -euo pipefail

if (( $# < 2 )); then
  echo "Uso: $0 ISO LOG [opções de qemu-smoke.py]" >&2
  exit 2
fi
iso=$1
log=$2
shift 2
scripts_dir=$(dirname "$0")
fixture=$(mktemp -d)
trap 'rm -rf -- "$fixture"' EXIT
mkdir "$fixture/root"

# O marcador só é escrito pelo novo executável, após validar argc.
as --64 -o "$fixture/probe.o" <<'ASM'
.globl _start
.text
_start:
    cmpq $0, (%rsp)
    jne bad_stack
    mov $1, %eax
    mov $1, %edi
    lea message(%rip), %rsi
    mov $message_len, %edx
    syscall
    xor %edi, %edi
    jmp exit
bad_stack:
    mov $1, %edi
exit:
    mov $60, %eax
    syscall
    ud2
message:
    .ascii "execve: ELF do disco executado em CPL3\n"
.set message_len, . - message
.section .note.GNU-stack,"",@progbits
ASM
ld -static -o "$fixture/root/carlio-disk-exec" "$fixture/probe.o"
chmod 755 "$fixture/root/carlio-disk-exec"

truncate -s 32M "$fixture/ext4"
mkfs.ext4 -q -F -O '^flex_bg,^metadata_csum_seed,^orphan_file' \
  -d "$fixture/root" "$fixture/ext4"
truncate -s 34M "$fixture/disk.raw"
sfdisk --quiet "$fixture/disk.raw" <<'MBR'
label: dos
start=2048,size=65536,type=83
MBR
dd if="$fixture/ext4" of="$fixture/disk.raw" bs=1M seek=1 conv=notrunc status=none
qemu-img convert -f raw -O qcow2 "$fixture/disk.raw" "$fixture/disk.qcow2"

python3 "$scripts_dir/qemu-smoke.py" --iso "$iso" --log "$log" \
  --disk-image "$fixture/disk.qcow2" --expect-disk-exec "$@"

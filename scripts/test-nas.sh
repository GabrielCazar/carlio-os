#!/usr/bin/env bash
set -euo pipefail

root_dir=$(cd "$(dirname "$0")/.." && pwd)
for tool in cargo ssh scp tar; do
  command -v "$tool" >/dev/null || { echo "Ferramenta ausente: $tool" >&2; exit 1; }
done
host=${CARLIO_TEST_SSH_TARGET:-nas-wifi}
vm_disk=${CARLIO_TEST_VM_DISK:-/home/gabriel/vms/carlioos/carlioos-40G.qcow2}
stamp=$(date -u +%Y%m%d-%H%M%S)
phase=${CARLIO_TEST_PHASE:-fase-1}
qemu_memory=${CARLIO_TEST_QEMU_MEMORY:-512M}
[[ $qemu_memory =~ ^[0-9]+[KMG]$ ]] || {
  echo "CARLIO_TEST_QEMU_MEMORY deve ter formato como 512M, 5G ou 32G." >&2
  exit 2
}
[[ $vm_disk == /* && $vm_disk != *"'"* && $vm_disk != *$'\n'* ]] || {
  echo "CARLIO_TEST_VM_DISK deve ser um caminho absoluto sem aspas simples ou quebras de linha." >&2
  exit 2
}
remote_dir="/tmp/carlioos-test-$stamp"
archive="$root_dir/build/carlioos-source-$stamp.tar.gz"
report_dir="$root_dir/reports/logs"
report="$report_dir/$phase-nas-$stamp.txt"
firmware=${CARLIO_TEST_OVMF:-/usr/share/edk2/x64/OVMF_CODE.4m.fd}
ssh_config_arg=()
if [[ -n ${CARLIO_TEST_SSH_CONFIG:-} ]]; then
  ssh_config_arg=(-F "$CARLIO_TEST_SSH_CONFIG")
fi
mkdir -p "$root_dir/build" "$report_dir"

cleanup() {
  ssh "${ssh_config_arg[@]}" -o BatchMode=yes "$host" "rm -rf '$remote_dir'" >/dev/null 2>&1 || true
  rm -f "$archive"
}
trap cleanup EXIT

test -s "$root_dir/build/carlio-os.iso" || {
  echo "ISO ausente; execute scripts/build-iso.sh antes." >&2
  exit 1
}

tar -czf "$archive" -C "$root_dir" \
  --exclude='./target' --exclude='./build' --exclude='./.git' .
ssh "${ssh_config_arg[@]}" -o BatchMode=yes "$host" "mkdir -p '$remote_dir/src'"
scp "${ssh_config_arg[@]}" -q "$archive" "$host:$remote_dir/source.tar.gz"
scp "${ssh_config_arg[@]}" -q "$root_dir/build/carlio-os.iso" "$host:$remote_dir/carlio-os.iso"

set +e
ssh "${ssh_config_arg[@]}" -o BatchMode=yes "$host" "set -e; mkdir -p '$remote_dir/cargo' '$remote_dir/target' '$remote_dir/tmp'; export CARGO_HOME='$remote_dir/cargo' CARGO_TARGET_DIR='$remote_dir/target' TMPDIR='$remote_dir/tmp'; test -r /dev/kvm; test -f '$firmware'; test -f '$vm_disk'; qemu-img info '$vm_disk'; tar -xzf '$remote_dir/source.tar.gz' -C '$remote_dir/src'; cd '$remote_dir/src'; cargo test --all-targets; cargo run --bin herm-fs -- status; python3 scripts/qemu-smoke.py --iso '$remote_dir/carlio-os.iso' --disk-image '$vm_disk' --accel kvm --memory '$qemu_memory' --timeout 25 --log '$remote_dir/qemu-kvm-bios.log'; python3 scripts/qemu-smoke.py --iso '$remote_dir/carlio-os.iso' --disk-image '$vm_disk' --accel kvm --firmware '$firmware' --memory '$qemu_memory' --timeout 35 --log '$remote_dir/qemu-kvm-uefi.log'" 2>&1 | tee "$report"
result=${PIPESTATUS[0]}
set -e

scp "${ssh_config_arg[@]}" -q "$host:$remote_dir/qemu-kvm-bios.log" "$report_dir/$phase-nas-$stamp-qemu-kvm-bios.log" 2>/dev/null || true
scp "${ssh_config_arg[@]}" -q "$host:$remote_dir/qemu-kvm-uefi.log" "$report_dir/$phase-nas-$stamp-qemu-kvm-uefi.log" 2>/dev/null || true

if [[ ${CARLIO_TEST_STORAGE:-0} == 1 && $result == 0 ]]; then
  set +e
  ssh "${ssh_config_arg[@]}" -o BatchMode=yes "$host" "export TMPDIR='$remote_dir/tmp'; cd '$remote_dir/src'; python3 scripts/test-ahci-qemu.py --iso '$remote_dir/carlio-os.iso' --disk-image '$vm_disk' --accel kvm --timeout 60 --log '$remote_dir/filesystem-qemu-kvm.log'" 2>&1 | tee -a "$report"
  storage_result=${PIPESTATUS[0]}
  set -e
  scp "${ssh_config_arg[@]}" -q "$host:$remote_dir/filesystem-qemu-kvm.log" "$report_dir/$phase-nas-$stamp-filesystem-qemu-kvm.log" 2>/dev/null || true
  if [[ $storage_result != 0 ]]; then
    result=$storage_result
  fi
fi

exit "$result"

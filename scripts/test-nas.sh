#!/usr/bin/env bash
set -euo pipefail

root_dir=$(cd "$(dirname "$0")/.." && pwd)
host=${CARLIO_TEST_SSH_TARGET:-nas-wifi}
stamp=$(date -u +%Y%m%d-%H%M%S)
phase=${CARLIO_TEST_PHASE:-fase-1}
remote_dir="/tmp/carlioos-test-$stamp"
archive="$root_dir/build/carlioos-source-$stamp.tar.gz"
report_dir="$root_dir/reports/logs"
report="$report_dir/$phase-nas-$stamp.txt"
firmware=${CARLIO_TEST_OVMF:-/usr/share/edk2/x64/OVMF_CODE.4m.fd}
mkdir -p "$root_dir/build" "$report_dir"

cleanup() {
  ssh -o BatchMode=yes "$host" "rm -rf '$remote_dir'" >/dev/null 2>&1 || true
  rm -f "$archive"
}
trap cleanup EXIT

test -s "$root_dir/build/carlio-os.iso" || {
  echo "ISO ausente; execute scripts/build-iso.sh antes." >&2
  exit 1
}

tar -czf "$archive" -C "$root_dir" \
  --exclude='./target' --exclude='./build' --exclude='./.git' .
ssh -o BatchMode=yes "$host" "mkdir -p '$remote_dir/src'"
scp -q "$archive" "$host:$remote_dir/source.tar.gz"
scp -q "$root_dir/build/carlio-os.iso" "$host:$remote_dir/carlio-os.iso"

set +e
ssh -o BatchMode=yes "$host" "set -e; test -r /dev/kvm; test -f '$firmware'; tar -xzf '$remote_dir/source.tar.gz' -C '$remote_dir/src'; cd '$remote_dir/src'; cargo test --all-targets; cargo run --bin herm-fs -- status; python3 scripts/qemu-smoke.py --iso '$remote_dir/carlio-os.iso' --accel kvm --timeout 25 --log '$remote_dir/qemu-kvm-bios.log'; python3 scripts/qemu-smoke.py --iso '$remote_dir/carlio-os.iso' --accel kvm --firmware '$firmware' --timeout 35 --log '$remote_dir/qemu-kvm-uefi.log'" 2>&1 | tee "$report"
result=${PIPESTATUS[0]}
set -e

scp -q "$host:$remote_dir/qemu-kvm-bios.log" "$report_dir/$phase-nas-$stamp-qemu-kvm-bios.log" 2>/dev/null || true
scp -q "$host:$remote_dir/qemu-kvm-uefi.log" "$report_dir/$phase-nas-$stamp-qemu-kvm-uefi.log" 2>/dev/null || true

if [[ ${CARLIO_TEST_STORAGE:-0} == 1 && $result == 0 ]]; then
  set +e
  ssh -o BatchMode=yes "$host" "cd '$remote_dir/src'; python3 scripts/test-ahci-qemu.py --iso '$remote_dir/carlio-os.iso' --accel kvm --timeout 60 --log '$remote_dir/filesystem-qemu-kvm.log'" 2>&1 | tee -a "$report"
  storage_result=${PIPESTATUS[0]}
  set -e
  scp -q "$host:$remote_dir/filesystem-qemu-kvm.log" "$report_dir/$phase-nas-$stamp-filesystem-qemu-kvm.log" 2>/dev/null || true
  if [[ $storage_result != 0 ]]; then
    result=$storage_result
  fi
fi

exit "$result"

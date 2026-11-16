#!/usr/bin/env bash
set -euo pipefail

root_dir=$(cd "$(dirname "$0")/.." && pwd)
cd "$root_dir"
for tool in cargo rustup python3 qemu-system-x86_64 limine xorriso; do
  command -v "$tool" >/dev/null || { echo "Ferramenta ausente: $tool" >&2; exit 1; }
done
stamp=$(date -u +%Y%m%d-%H%M%S)
phase=${CARLIO_TEST_PHASE:-fase-local}
qemu_memory=${CARLIO_TEST_QEMU_MEMORY:-512M}
[[ $qemu_memory =~ ^[0-9]+[KMG]$ ]] || {
  echo "CARLIO_TEST_QEMU_MEMORY deve ter formato como 512M, 5G ou 32G." >&2
  exit 2
}
log_dir="$root_dir/reports/logs"
mkdir -p "$log_dir"

cargo fmt --check
cargo test --all-targets > "$log_dir/$phase-host-$stamp.txt" 2>&1
scripts/build-iso.sh
python3 scripts/qemu-smoke.py --iso build/carlio-os.iso --accel tcg --memory "$qemu_memory" \
  --timeout 25 --log "$log_dir/$phase-qemu-bios-$stamp.log"

firmware=${CARLIO_TEST_OVMF:-/usr/share/edk2/x64/OVMF_CODE.4m.fd}
if [[ -f "$firmware" ]]; then
  python3 scripts/qemu-smoke.py --iso build/carlio-os.iso --accel tcg --memory "$qemu_memory" \
    --firmware "$firmware" --timeout 35 \
    --log "$log_dir/$phase-qemu-uefi-$stamp.log"
else
  printf 'UEFI ignorado: firmware OVMF não encontrado em %s\n' "$firmware" >&2
fi

printf 'Validação local concluída; logs: %s\n' "$log_dir"

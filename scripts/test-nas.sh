#!/usr/bin/env bash
set -euo pipefail

root_dir=$(cd "$(dirname "$0")/.." && pwd)
for tool in cargo ssh scp tar; do
  command -v "$tool" >/dev/null || { echo "Ferramenta ausente: $tool" >&2; exit 1; }
done
host=${CARLIO_TEST_SSH_TARGET:-nas-manjaro}
stamp=$(date -u +%Y%m%d-%H%M%S)
phase=${CARLIO_TEST_PHASE:-fase-1}
iso=${CARLIO_TEST_ISO:-$root_dir/build/carlio-os.iso}
qemu_only=${CARLIO_TEST_QEMU_ONLY:-0}
[[ $qemu_only == 0 || $qemu_only == 1 ]] || {
  echo "CARLIO_TEST_QEMU_ONLY deve ser 0 ou 1." >&2
  exit 2
}
qemu_memory=${CARLIO_TEST_QEMU_MEMORY:-512M}
[[ $qemu_memory =~ ^[0-9]+[KMG]$ ]] || {
  echo "CARLIO_TEST_QEMU_MEMORY deve ter formato como 512M, 5G ou 32G." >&2
  exit 2
}
remote_dir="/tmp/carlioos-test-$stamp-$$"
vm_disk=${CARLIO_TEST_VM_DISK:-$remote_dir/disk.qcow2}
disk_size=40G
if [[ ${CARLIO_TEST_STORAGE_2T:-0} == 1 ]]; then
  if [[ ${CARLIO_TEST_STORAGE:-0} != 1 || -n ${CARLIO_TEST_VM_DISK:-} ]]; then
    echo "CARLIO_TEST_STORAGE_2T requer CARLIO_TEST_STORAGE=1 e o disco descartável do runner." >&2
    exit 2
  fi
  disk_size=2T
fi
[[ $vm_disk == /* && $vm_disk != *"'"* && $vm_disk != *$'\n'* ]] || {
  echo "CARLIO_TEST_VM_DISK deve ser um caminho absoluto sem aspas simples ou quebras de linha." >&2
  exit 2
}
archive="$root_dir/build/carlioos-source-$stamp.tar.gz"
report_dir="$root_dir/reports/logs"
report="$report_dir/$phase-nas-$stamp.txt"
firmware=${CARLIO_TEST_OVMF:-/usr/share/edk2/x64/OVMF_CODE.4m.fd}
ssh_config_arg=()
if [[ -n ${CARLIO_TEST_SSH_CONFIG:-} ]]; then
  ssh_config_arg=(-F "$CARLIO_TEST_SSH_CONFIG")
elif [[ $host == nas-manjaro && -f $HOME/.ssh/config_lab ]]; then
  ssh_config_arg=(-F "$HOME/.ssh/config_lab")
fi
mkdir -p "$root_dir/build" "$report_dir"

cleanup() {
  ssh "${ssh_config_arg[@]}" -o BatchMode=yes "$host" "rm -rf '$remote_dir'" >/dev/null 2>&1 || true
  rm -f "$archive"
}
trap cleanup EXIT

test -s "$iso" || {
  echo "ISO ausente em $iso; execute scripts/build-iso.sh antes." >&2
  exit 1
}

tar -czf "$archive" -C "$root_dir" \
  --exclude='./target' --exclude='./build' --exclude='./.git' \
  --exclude='./.agents' --exclude='./.codex' --exclude='./.aws' .
ssh "${ssh_config_arg[@]}" -o BatchMode=yes "$host" "mkdir -p '$remote_dir/src'"
scp "${ssh_config_arg[@]}" -q "$archive" "$host:$remote_dir/source.tar.gz"
scp "${ssh_config_arg[@]}" -q -- "$iso" "$host:$remote_dir/carlio-os.iso"

set +e
ssh "${ssh_config_arg[@]}" -o BatchMode=yes "$host" "
  set -e
  mkdir -p '$remote_dir/cargo' '$remote_dir/target' '$remote_dir/tmp'
  export PATH=\"\$HOME/.cargo/bin:\$PATH\"
  export CARGO_HOME='$remote_dir/cargo' CARGO_TARGET_DIR='$remote_dir/target' TMPDIR='$remote_dir/tmp'
  if ! test -r /dev/kvm || ! test -w /dev/kvm; then
    echo 'O usuário SSH precisa de acesso de leitura e escrita a /dev/kvm.' >&2
    exit 1
  fi
  test -f '$firmware'
  if test '$vm_disk' = '$remote_dir/disk.qcow2'; then
    qemu-img create -f qcow2 '$vm_disk' '$disk_size'
  fi
  qemu-img info '$vm_disk'
  tar -xzf '$remote_dir/source.tar.gz' -C '$remote_dir/src'
  cd '$remote_dir/src'
  if test '$qemu_only' = 1; then
    echo 'Modo QEMU: compilação e testes Cargo no NAS omitidos.'
  else
    offline=\$HOME/.local/share/carlioos-test/toolchain
    if test -x \"\$offline/nightly-x86_64-unknown-linux-gnu/bin/cargo\"; then
      export PATH=\"\$offline/nightly-x86_64-unknown-linux-gnu/bin:\$offline/gcc/bin:\$PATH\"
      export LD_LIBRARY_PATH=\"\$offline/gcc/compat\${LD_LIBRARY_PATH:+:\$LD_LIBRARY_PATH}\"
      export CARGO_NET_OFFLINE=true
      printf '[source.crates-io]\\nreplace-with = \"vendor\"\\n[source.vendor]\\ndirectory = \"%s\"\\n' \"\$offline/vendor\" > \"\$CARGO_HOME/config.toml\"
    fi
    cargo test --locked --all-targets
    cargo run --locked --bin herm-fs -- status
  fi
  boot_failed=0
  python3 scripts/qemu-smoke.py --iso '$remote_dir/carlio-os.iso' --disk-image '$vm_disk' --accel kvm --memory '$qemu_memory' --timeout 25 --log '$remote_dir/qemu-kvm-bios.log' || boot_failed=1
  python3 scripts/qemu-smoke.py --iso '$remote_dir/carlio-os.iso' --disk-image '$vm_disk' --accel kvm --firmware '$firmware' --memory '$qemu_memory' --timeout 35 --log '$remote_dir/qemu-kvm-uefi.log' || boot_failed=1
  exit \$boot_failed
" 2>&1 | tee "$report"
result=${PIPESTATUS[0]}
set -e

scp "${ssh_config_arg[@]}" -q "$host:$remote_dir/qemu-kvm-bios.log" "$report_dir/$phase-nas-$stamp-qemu-kvm-bios.log" 2>/dev/null || true
scp "${ssh_config_arg[@]}" -q "$host:$remote_dir/qemu-kvm-uefi.log" "$report_dir/$phase-nas-$stamp-qemu-kvm-uefi.log" 2>/dev/null || true

if [[ ${CARLIO_TEST_STORAGE:-0} == 1 && $result == 0 ]]; then
  set +e
  ssh "${ssh_config_arg[@]}" -o BatchMode=yes "$host" "export TMPDIR='$remote_dir/tmp'; cd '$remote_dir/src'; python3 scripts/test-ahci-qemu.py --iso '$remote_dir/carlio-os.iso' --disk-image '$vm_disk' --accel kvm --timeout 60 --log '$remote_dir/filesystem-qemu-kvm.log'" 2>&1 | tee -a "$report"
  storage_result=${PIPESTATUS[0]}
  set -e
  storage_log="$report_dir/$phase-nas-$stamp-filesystem-qemu-kvm.log"
  scp "${ssh_config_arg[@]}" -q "$host:$remote_dir/filesystem-qemu-kvm.log" "$storage_log" 2>/dev/null || true
  if [[ $disk_size == 2T ]] && ! grep -Fq 'AHCI: limite de 2 TiB verificado.' "$storage_log"; then
    echo "A confirmação do teste AHCI de 2 TiB não apareceu no log." | tee -a "$report"
    storage_result=1
  fi
  if [[ $storage_result != 0 ]]; then
    result=$storage_result
  else
    set +e
    ssh "${ssh_config_arg[@]}" -o BatchMode=yes "$host" "export TMPDIR='$remote_dir/tmp'; cd '$remote_dir/src'; python3 scripts/test-ahci-qemu.py --iso '$remote_dir/carlio-os.iso' --remove-controller --accel kvm --timeout 60 --log '$remote_dir/ahci-remocao-qemu-kvm.log'" 2>&1 | tee -a "$report"
    result=${PIPESTATUS[0]}
    set -e
    scp "${ssh_config_arg[@]}" -q "$host:$remote_dir/ahci-remocao-qemu-kvm.log" "$report_dir/$phase-nas-$stamp-ahci-remocao-qemu-kvm.log" 2>/dev/null || true
  fi
fi

exit "$result"

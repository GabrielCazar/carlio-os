#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
for tool in cargo rustup limine xorriso; do
  command -v "$tool" >/dev/null || { echo "Ferramenta ausente: $tool" >&2; exit 1; }
done

if ! rustup run nightly rustc --version >/dev/null 2>&1; then
  echo "Toolchain nightly ausente; instale nightly com rust-src e x86_64-unknown-none (ver README.md)." >&2
  exit 1
fi
if ! rustup target list --installed --toolchain nightly | awk '$0 == "x86_64-unknown-none" { found = 1 } END { exit !found }'; then
  echo "Alvo x86_64-unknown-none ausente no nightly; instale-o (ver README.md)." >&2
  exit 1
fi

build_args=(build --release --bin carlio-kernel --target x86_64-unknown-none)
features=()
if [[ ${CARLIO_CRASH_TEST:-0} == 1 ]]; then
  features+=(crash-test)
fi
if [[ ${CARLIO_STORAGE_TEST:-0} == 1 ]]; then
  features+=(storage-test)
fi
if [[ ${CARLIO_STACK_WATERMARK:-0} == 1 ]]; then
  features+=(stack-watermark)
fi
if [[ ${CARLIO_SMP_STRESS_TEST:-0} == 1 ]]; then
  features+=(smp-stress-test)
fi
if [[ ${CARLIO_DISK_EXEC_TEST:-0} == 1 ]]; then
  features+=(disk-exec-test)
fi
if ((${#features[@]})); then
  feature_list=$(IFS=,; echo "${features[*]}")
  build_args+=(--features "$feature_list")
fi
if [[ -n ${RUSTUP_HOME:-} || -n ${CARGO_HOME:-} ]]; then
  # Honor isolated toolchain/cache locations when the caller configured them.
  # Otherwise use rustup's normal installation; forcing an empty /tmp rustup
  # home made a correctly provisioned developer machine fail to build.
  if [[ -n ${RUSTUP_HOME:-} && -n ${CARGO_HOME:-} ]]; then
    RUSTUP_HOME="$RUSTUP_HOME" CARGO_HOME="$CARGO_HOME" \
      rustup run nightly cargo "${build_args[@]}"
  elif [[ -n ${RUSTUP_HOME:-} ]]; then
    RUSTUP_HOME="$RUSTUP_HOME" rustup run nightly cargo "${build_args[@]}"
  else
    CARGO_HOME="$CARGO_HOME" rustup run nightly cargo "${build_args[@]}"
  fi
else
  rustup run nightly cargo "${build_args[@]}"
fi
limine_root=/usr/share/limine
test -f "$limine_root/limine-bios.sys" || { echo "Arquivos do Limine ausentes em $limine_root" >&2; exit 1; }

rm -rf build/iso-root
mkdir -p build/iso-root/boot build/iso-root/EFI/BOOT
cp target/x86_64-unknown-none/release/carlio-kernel build/iso-root/boot/carlio-kernel
cp limine.conf build/iso-root/limine.conf
cp "$limine_root/limine-bios.sys" "$limine_root/limine-bios-cd.bin" \
   "$limine_root/limine-uefi-cd.bin" build/iso-root/boot/
cp "$limine_root/BOOTX64.EFI" build/iso-root/EFI/BOOT/
xorriso -as mkisofs -R -r -J -b boot/limine-bios-cd.bin \
  -no-emul-boot -boot-load-size 4 -boot-info-table --efi-boot boot/limine-uefi-cd.bin \
  -efi-boot-part --efi-boot-image --protective-msdos-label \
  build/iso-root -o build/carlio-os.iso
limine bios-install build/carlio-os.iso
echo "Imagem criada: build/carlio-os.iso"

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
image=build/carlio-os.iso
if [[ ${CARLIO_PHYSICAL_TEST:-0} == 1 ]]; then
  features+=(physical-test)
  image=build/carlio-os-physical-test.iso
fi
if [[ ${CARLIO_DIRCACHE:-0} == 1 ]]; then
  features+=(dircache)
  image=${image%.iso}-dircache.iso
fi
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
if [[ ${CARLIO_DISK_READ_TEST:-0} == 1 ]]; then
  features+=(disk-read-test)
fi
lockdown_mode=${CARLIO_LOCKDOWN:-none}
case "$lockdown_mode" in
  none|integrity|confidentiality) ;;
  *) echo "CARLIO_LOCKDOWN inválido: $lockdown_mode" >&2; exit 1 ;;
esac
if [[ $lockdown_mode != none ]]; then
  image=${image%.iso}-lockdown-${lockdown_mode}.iso
fi
if [[ ${CARLIO_SECURE_BOOT_STAGE:-0} == 1 ]]; then
  if [[ $lockdown_mode == none || ${CARLIO_CRASH_TEST:-0} == 1 || ${CARLIO_STORAGE_TEST:-0} == 1 ]]; then
    echo "O estágio Secure Boot exige lockdown e exclui crash-test/storage-test." >&2
    exit 1
  fi
  command -v b2sum >/dev/null || { echo "Ferramenta ausente: b2sum" >&2; exit 1; }
  case ${CARLIO_LOCKDOWN_FLOOR:-none} in
    none) export CARLIO_LOCKDOWN_FLOOR=$lockdown_mode ;;
  esac
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
if [[ $lockdown_mode != none ]]; then
  printf '    cmdline: lockdown=%s\n' "$lockdown_mode" >> build/iso-root/limine.conf
fi
cp "$limine_root/limine-bios.sys" "$limine_root/limine-bios-cd.bin" \
   "$limine_root/limine-uefi-cd.bin" build/iso-root/boot/
cp "$limine_root/BOOTX64.EFI" build/iso-root/EFI/BOOT/
if [[ ${CARLIO_SECURE_BOOT_STAGE:-0} == 1 ]]; then
  stage=${image%.iso}-uefi-stage
  mkdir -p "$stage/EFI/BOOT" "$stage/boot"
  cp build/iso-root/boot/carlio-kernel "$stage/boot/"
  cp build/iso-root/EFI/BOOT/BOOTX64.EFI "$stage/EFI/BOOT/"
  read -r kernel_hash _ < <(b2sum "$stage/boot/carlio-kernel")
  sed "s|/boot/carlio-kernel$|/boot/carlio-kernel#$kernel_hash|" build/iso-root/limine.conf > "$stage/limine.conf"
  read -r config_hash _ < <(b2sum "$stage/limine.conf")
  limine enroll-config "$stage/EFI/BOOT/BOOTX64.EFI" "$config_hash"
  echo "Estágio UEFI criado em $stage. Assine EFI/BOOT/BOOTX64.EFI antes de instalar."
fi
xorriso -as mkisofs -R -r -J -b boot/limine-bios-cd.bin \
  -no-emul-boot -boot-load-size 4 -boot-info-table --efi-boot boot/limine-uefi-cd.bin \
  -efi-boot-part --efi-boot-image --protective-msdos-label \
  build/iso-root -o "$image"
limine bios-install "$image"
echo "Imagem criada: $image"

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
fixture=$(mktemp -d -t carlioos-disk-exec.XXXXXXXX)
trap 'rm -rf -- "$fixture"' EXIT
mkdir "$fixture/root"

linux_rootfs=${CARLIO_TEST_LINUX_ROOTFS:-}
pegasus_root=${CARLIO_TEST_PEGASUS_ROOT:-}
if [[ -n $pegasus_root && ( -n $linux_rootfs || ${CARLIO_TEST_EXEC_FS:-ext4} != ext4 || ${CARLIO_TEST_EXEC_INTERP:-0} == 1 ) ]]; then
  echo "CARLIO_TEST_PEGASUS_ROOT exige ext4, sem rootfs ou interpretador alternativo." >&2
  exit 2
fi
if [[ -n $linux_rootfs && ( ${CARLIO_TEST_EXEC_FS:-ext4} != ext4 || ${CARLIO_TEST_EXEC_INTERP:-0} == 1 ) ]]; then
  echo "CARLIO_TEST_LINUX_ROOTFS exige ext4 e usa o próprio interpretador do rootfs." >&2
  exit 2
fi

exec_args=(-static)
if [[ ${CARLIO_TEST_EXEC_INTERP:-0} == 1 ]]; then
  [[ ${CARLIO_TEST_EXEC_FS:-ext4} == ext4 ]] || {
    echo "CARLIO_TEST_EXEC_INTERP exige ext4." >&2
    exit 2
  }
  as --64 -o "$fixture/interpreter.o" "$scripts_dir/fixtures/elf-interpreter.s"
  ld -pie --no-dynamic-linker -o "$fixture/root/carlio-interpreter" "$fixture/interpreter.o"
  truncate -s 256K "$fixture/root/carlio-interpreter"
  exec_args=(-pie --dynamic-linker /carlio-interpreter)
fi

case ${CARLIO_TEST_EXEC_FS:-ext4} in
  ext4)
    if [[ -n $pegasus_root ]]; then
      cargo +stable rustc --manifest-path "$pegasus_root/Cargo.toml" --locked \
        --package pegasusd --features directory-diagnostic --release --target x86_64-unknown-linux-gnu \
        --target-dir "$fixture/pegasus-target" -- -C strip=symbols -C link-arg=-Wl,-rpath,/lib64
      install -m 0755 "$fixture/pegasus-target/x86_64-unknown-linux-gnu/release/pegasusd" \
        "$fixture/root/carlio-disk-exec"
      loader=ld-linux-x86-64.so.2
      install -D -m 0755 "$(cc -print-file-name="$loader")" "$fixture/root/lib64/$loader"
      for library in libc.so.6 libgcc_s.so.1; do
        library_path=$(cc -print-file-name="$library")
        install -D -m 0644 "$library_path" "$fixture/root/lib64/$library"
      done
      expect=--expect-pegasus
    elif [[ -n $linux_rootfs ]]; then
      # O guest ainda precisa de arquivos regulares nos caminhos do loader.
      library_dir=usr/lib
      guest_library_dir=usr/lib
      if [[ -f $linux_rootfs/usr/lib/x86_64-linux-gnu/libc.so.6 ]]; then
        library_dir=usr/lib/x86_64-linux-gnu
        guest_library_dir=lib/x86_64-linux-gnu
      fi
      install -D -m 0755 "$linux_rootfs/usr/bin/uname" "$fixture/root/carlio-disk-exec"
      install -D -m 0755 "$linux_rootfs/$library_dir/ld-linux-x86-64.so.2" \
        "$fixture/root/lib64/ld-linux-x86-64.so.2"
      install -D -m 0644 "$linux_rootfs/$library_dir/libc.so.6" \
        "$fixture/root/$guest_library_dir/libc.so.6"
      expect=--expect-linux-uname
    else
      as --64 -o "$fixture/probe.o" "$scripts_dir/fixtures/syscall-vfs.s"
      ld "${exec_args[@]}" -o "$fixture/root/carlio-disk-exec" "$fixture/probe.o"
      truncate -s 32K "$fixture/root/carlio-disk-exec"
      chmod 755 "$fixture/root/carlio-disk-exec"
      printf A > "$fixture/root/mmap-window"
      printf B | dd of="$fixture/root/mmap-window" bs=1 seek=65536 conv=notrunc status=none
      mkdir -m 0700 "$fixture/root/carlio-privado"
      cp "$fixture/root/carlio-disk-exec" "$fixture/root/carlio-privado/probe"
      expect=--expect-disk-exec
    fi
    truncate -s 64M "$fixture/volume"
    mkfs.ext4 -q -F -O '^flex_bg,^metadata_csum_seed,^orphan_file' \
      -d "$fixture/root" "$fixture/volume"
    if [[ -z $linux_rootfs && -z $pegasus_root ]]; then
      debugfs -w -R "set_inode_field /carlio-privado uid 0" "$fixture/volume"
    fi
    partition_type=83
    ;;
  fat32)
    truncate -s 64M "$fixture/volume"
    mkfs.fat -F 32 "$fixture/volume"
    printf "CarlioOS SATA FAT32\n" > "$fixture/probe.bin"
    mcopy -i "$fixture/volume" "$fixture/probe.bin" ::/probe.bin
    partition_type=c
    expect=--expect-disk-read
    ;;
  *)
    echo "CARLIO_TEST_EXEC_FS deve ser ext4 ou fat32." >&2
    exit 2
    ;;
esac

truncate -s 65M "$fixture/disk.raw"
sfdisk --quiet "$fixture/disk.raw" <<MBR
label: dos
start=2048,size=131072,type=$partition_type
MBR
dd if="$fixture/volume" of="$fixture/disk.raw" bs=1M seek=1 conv=notrunc status=none
qemu-img convert -f raw -O qcow2 "$fixture/disk.raw" "$fixture/disk.qcow2"

python3 "$scripts_dir/qemu-smoke.py" --iso "$iso" --log "$log" \
  --disk-image "$fixture/disk.qcow2" "$expect" "$@"

if [[ ${CARLIO_TEST_EXEC_INTERP:-0} == 1 ]]; then
  if ! grep -Fq 'execve: PT_INTERP e auxv verificados em CPL3' "$log"; then
    echo "Execução de PT_INTERP ou validação de auxv ausente. Log: $log" >&2
    exit 1
  fi
fi

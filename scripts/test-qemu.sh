#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
test -f build/carlio-os.iso || { echo "Execute scripts/build-iso.sh primeiro" >&2; exit 1; }
exec qemu-system-x86_64 -machine q35 -m 512M -cdrom build/carlio-os.iso \
  -boot d -serial stdio -display none -no-reboot -no-shutdown

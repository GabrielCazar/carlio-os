#!/usr/bin/env python3
"""Create a new sparse CarlioOS persistence disk image for installation."""

import argparse
from pathlib import Path

MAGIC = b"CARLIO-PERSIST-03"
MAX_SWAP_PAGES = 64
PAGE_SIZE = 4096
MIB = 1024 * 1024


def provision(
    path: Path, size_mib: int, *, vfs: bool = False, swap_pages: int = 0
) -> None:
    if not 0 <= swap_pages <= MAX_SWAP_PAGES:
        raise ValueError(f"O swap deve ter entre 0 e {MAX_SWAP_PAGES} páginas.")
    layout_mib = 4 if vfs else 2
    minimum = layout_mib + (swap_pages * PAGE_SIZE + MIB - 1) // MIB
    if not minimum <= size_mib <= 2 * 1024 * 1024:
        raise ValueError(f"O tamanho deve estar entre {minimum} MiB e 2 TiB.")
    header = (MAGIC + bytes((int(vfs), swap_pages))).ljust(512, b"\0")
    # Exclusive creation protects existing images and rules out block devices.
    with path.open("xb") as disk:
        disk.truncate(size_mib * MIB)
        disk.write(header)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", type=Path)
    parser.add_argument("--size-mib", type=int, default=16 * 1024)
    parser.add_argument("--vfs", action="store_true", help="reserva espaço para o overlay persistente")
    parser.add_argument("--swap-pages", type=int, default=0,
                        help="páginas de 4 KiB para swap experimental (0–64, padrão 0)")
    args = parser.parse_args()
    try:
        provision(args.image, args.size_mib, vfs=args.vfs, swap_pages=args.swap_pages)
    except (OSError, ValueError) as error:
        parser.exit(1, f"{error}\n")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Create a new sparse CarlioOS persistence disk image for installation."""

import argparse
from pathlib import Path

HEADER = b"CARLIO-PERSIST-01".ljust(512, b"\0")
MIB = 1024 * 1024


def provision(path: Path, size_mib: int, *, vfs: bool = False) -> None:
    minimum = 5 if vfs else 3
    if not minimum <= size_mib <= 2 * 1024 * 1024:
        raise ValueError(f"O tamanho deve estar entre {minimum} MiB e 2 TiB.")
    # Exclusive creation protects existing images and rules out block devices.
    with path.open("xb") as disk:
        disk.truncate(size_mib * MIB)
        disk.write(HEADER.replace(b"-01", b"-02") if vfs else HEADER)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", type=Path)
    parser.add_argument("--size-mib", type=int, default=16 * 1024)
    parser.add_argument("--vfs", action="store_true", help="reserva espaço para o overlay persistente")
    args = parser.parse_args()
    try:
        provision(args.image, args.size_mib, vfs=args.vfs)
    except (OSError, ValueError) as error:
        parser.exit(1, f"{error}\n")


if __name__ == "__main__":
    main()

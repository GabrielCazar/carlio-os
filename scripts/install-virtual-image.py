#!/usr/bin/env python3
"""Prepara um disco virtual inicializável a partir de uma ISO híbrida do CarlioOS."""

import argparse
import hashlib
import os
import shutil
import stat
import tempfile
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("iso", type=Path)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--sha256", help="SHA-256 obtido de um manifesto verificado")
    parser.add_argument("--apply", action="store_true", help="grava após confirmação")
    args = parser.parse_args()
    if args.apply and args.sha256 is None:
        parser.error("--apply exige --sha256 de fonte confiável")
    destination = args.destination.parent.resolve() / args.destination.name
    try:
        if not stat.S_ISREG(args.iso.stat().st_mode):
            raise ValueError("A origem precisa ser um arquivo ISO regular.")
        if os.path.lexists(destination):
            raise FileExistsError(f"O destino já existe: {destination}")
        with args.iso.open("rb") as source:
            metadata = os.fstat(source.fileno())
            if not stat.S_ISREG(metadata.st_mode):
                raise ValueError("A origem precisa ser um arquivo ISO regular.")
            source.seek(510)
            boot_signature = source.read(2)
            source.seek(16 * 2048)
            if boot_signature != b"\x55\xaa" or source.read(7) != b"\x01CD001\x01":
                raise ValueError("A origem não tem as assinaturas esperadas de uma ISO híbrida.")
            source.seek(0)
            digest = hashlib.file_digest(source, "sha256").hexdigest()
            if args.sha256 is not None and digest != args.sha256.lower():
                raise ValueError("SHA-256 divergente. Operação cancelada.")
            print(f"Origem    {args.iso.resolve()}")
            print(f"Destino   {destination}")
            print(f"Tamanho   {metadata.st_size} bytes")
            print(f"SHA-256   {digest}")
            if not args.apply:
                return
            if input("Digite o caminho de destino exibido para confirmar.\n> ") != str(destination):
                raise ValueError("Confirmação divergente. Operação cancelada.")
            source.seek(0)
            parent_fd = os.open(destination.parent, os.O_RDONLY | os.O_DIRECTORY)
            try:
                with tempfile.NamedTemporaryFile(
                    dir=destination.parent, prefix=".carlio-", mode="w+b"
                ) as temporary:
                    shutil.copyfileobj(source, temporary)
                    temporary.flush()
                    temporary.seek(0)
                    if hashlib.file_digest(temporary, "sha256").hexdigest() != digest:
                        raise ValueError("A origem mudou durante a cópia.")
                    os.fsync(temporary.fileno())
                    # Publish without overwriting a destination created concurrently.
                    os.link(temporary.name, destination.name, dst_dir_fd=parent_fd)
                # Ensure the verified destination survives a restart after success.
                os.fsync(parent_fd)
            finally:
                os.close(parent_fd)
    except (OSError, ValueError, EOFError) as error:
        parser.exit(1, f"{error}\n")
    print(f"Imagem virtual criada em {destination}")


if __name__ == "__main__":
    main()

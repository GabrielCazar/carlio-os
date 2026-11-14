#!/usr/bin/env python3
"""Teste automatizado do terminal serial do CarlioOS em uma VM QEMU."""

import argparse
import os
import selectors
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--iso", required=True, type=Path)
    parser.add_argument("--accel", default="tcg", choices=("tcg", "kvm"))
    parser.add_argument("--firmware", type=Path, help="Imagem OVMF para testar UEFI")
    parser.add_argument("--log", required=True, type=Path)
    parser.add_argument("--timeout", type=int, default=25)
    args = parser.parse_args()

    command = [
        "qemu-system-x86_64",
        "-machine", "q35",
        "-accel", args.accel,
        "-m", "512M",
        "-cdrom", str(args.iso.resolve()),
        "-boot", "d",
        "-serial", "stdio",
        "-monitor", "none",
        "-display", "none",
        "-no-reboot",
        "-no-shutdown",
    ]
    vars_copy = None
    if args.firmware:
        code_image = args.firmware.resolve()
        vars_image = code_image.with_name(code_image.name.replace("CODE", "VARS", 1))
        if not vars_image.is_file():
            parser.error(f"Imagem OVMF de variáveis ausente: {vars_image}")
        args.log.parent.mkdir(parents=True, exist_ok=True)
        fd, vars_copy = tempfile.mkstemp(prefix="carlio-ovmf-vars-", suffix=".fd", dir=args.log.parent)
        os.close(fd)
        shutil.copyfile(vars_image, vars_copy)
        command.extend((
            "-drive", f"if=pflash,format=raw,unit=0,readonly=on,file={code_image}",
            "-drive", f"if=pflash,format=raw,unit=1,file={vars_copy}",
        ))

    process = subprocess.Popen(
        command,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        bufsize=0,
    )
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ, "serial")
    selector.register(process.stderr, selectors.EVENT_READ, "qemu")
    output = bytearray()
    errors = bytearray()
    stage = 0
    checks = (
        (b"Digite 'ajuda'", b"ajuda\r"),
        ("Comandos: ajuda, sobre, usuario, permissoes, erros, limpar, parar.".encode(), b"erros\r"),
        (b"Erros registrados:", b"sobre\r"),
        ("carlIO OS | Criado por João G.R.Santos".encode(), None),
        ("Sistema operacional experimental x86_64 escrito em Rust.".encode(), b"parar\r"),
        ("Processador parado.".encode(), None),
    )
    deadline = time.monotonic() + args.timeout

    try:
        while stage < len(checks) and time.monotonic() < deadline:
            for key, _ in selector.select(timeout=0.25):
                chunk = os.read(key.fileobj.fileno(), 4096)
                if not chunk:
                    continue
                if key.data == "serial":
                    output.extend(chunk)
                    haystack = bytes(output)
                else:
                    errors.extend(chunk)
                    haystack = bytes(errors)
                expected, response = checks[stage]
                if expected in haystack:
                    if response:
                        process.stdin.write(response)
                        process.stdin.flush()
                    stage += 1
            if process.poll() is not None and stage < len(checks):
                break
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        args.log.parent.mkdir(parents=True, exist_ok=True)
        args.log.write_bytes(output + b"\n--- mensagens do QEMU ---\n" + errors)
        selector.close()
        if vars_copy and os.path.exists(vars_copy):
            os.unlink(vars_copy)

    if stage != len(checks):
        sys.stderr.write(f"Falha no teste QEMU: etapa {stage + 1}/{len(checks)}. Log: {args.log}\n")
        sys.stderr.write(output.decode("utf-8", errors="replace"))
        sys.stderr.write(errors.decode("utf-8", errors="replace"))
        return 1

    if any(signature not in output for signature in (
        b"SATA/AHCI", b" v\xc3\xaddeo", b" rede",
        "Criado por João G.R.Santos".encode(),
    )):
        sys.stderr.write(f"Inventario PCI ou tela inicial incompletos. Log: {args.log}\n")
        sys.stderr.write(output.decode("utf-8", errors="replace"))
        return 1

    print(output.decode("utf-8", errors="replace"), end="")
    print(f"Teste QEMU aprovado ({args.accel}); log: {args.log}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Testa AHCI IDENTIFY/READ/WRITE em uma imagem raw descartável do QEMU."""

import argparse
import os
import selectors
import subprocess
import sys
import tempfile
import time
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--iso", required=True, type=Path)
    parser.add_argument("--log", required=True, type=Path)
    parser.add_argument("--timeout", type=int, default=30)
    parser.add_argument("--accel", choices=("tcg", "kvm"), default="tcg",
                        help="acelerador do QEMU (KVM para NAS; TCG como padrão local)")
    parser.add_argument("--without-disk", action="store_true",
                        help="verifica ausência de dispositivo e não envia comandos de escrita")
    args = parser.parse_args()

    disk = None
    if not args.without_disk:
        # Respeita TMPDIR para permitir confinar imagens descartáveis ao sandbox da execução.
        disk = tempfile.NamedTemporaryFile(prefix="carlio-ahci-disposable-", suffix=".img", delete=False)
        disk.truncate(16 * 1024 * 1024)
        disk.close()
    qemu = [
        "qemu-system-x86_64", "-machine", "q35", "-accel", args.accel, "-m", "512M",
        "-smp", "4",
        "-cdrom", str(args.iso.resolve()),
        "-boot", "d", "-serial", "stdio", "-monitor", "none", "-display", "none",
        "-no-reboot", "-no-shutdown",
    ]
    if disk:
        drive = f"file={disk.name},format=raw,if=ide,index=0"
        qemu[7:7] = ["-drive", drive]
    process = subprocess.Popen(qemu, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, bufsize=0)
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ, "serial")
    selector.register(process.stderr, selectors.EVENT_READ, "qemu")
    serial = bytearray()
    errors = bytearray()
    sent = False
    probe_sent = False
    fs_sent = False
    fs_passed_once = False
    disk_test_passed = False
    passed = False
    deadline = time.monotonic() + args.timeout

    def has_complete_marker(marker: bytes) -> bool:
        return any(line.endswith(b"\n") and marker in line
                   for line in serial.splitlines(keepends=True))

    try:
        while time.monotonic() < deadline:
            for key, _ in selector.select(timeout=0.2):
                chunk = os.read(key.fileobj.fileno(), 4096)
                if not chunk:
                    continue
                if key.data == "serial":
                    serial.extend(chunk)
                    if (not probe_sent and
                            b"ABI Linux: probe CPL3 aguardando byte de entrada serial." in serial):
                        process.stdin.write(b"x")
                        process.stdin.flush()
                        probe_sent = True
                    if not args.without_disk and not sent and b"carlio> " in serial:
                        process.stdin.write(b"fs-teste\r")
                        process.stdin.flush()
                        sent = True
                    fs_pass_marker = b"Hermione: volume persistente, espelhamento, journal/commit, remontagem e scrub passaram.\r\n"
                    disk_pass_marker = b"AHCI: IDENTIFY, WRITE, READ e erro do controlador verificados.\r\n"
                    if not args.without_disk and fs_pass_marker in serial and not fs_passed_once:
                        fs_passed_once = True
                        process.stdin.write(b"diskteste\r")
                        process.stdin.flush()
                    if not args.without_disk and disk_pass_marker in serial and not disk_test_passed:
                        disk_test_passed = True
                        process.stdin.write(b"fs-teste\r")
                        process.stdin.flush()
                        fs_sent = True
                    if (not args.without_disk and fs_sent and disk_test_passed
                            and serial.count(fs_pass_marker) >= 2):
                        passed = True
                        return 0
                    absent_line = "AHCI QEMU indisponível: dispositivo nao encontrado.\r\n".encode()
                    if (args.without_disk and absent_line in serial and
                            b"Digite 'ajuda' para ver os comandos.\r\n" in serial):
                        passed = True
                        return 0
                    if (has_complete_marker(b"AHCI: teste falhou:")
                            or (not args.without_disk and "AHCI indisponível:".encode() in serial)):
                        break
                else:
                    errors.extend(chunk)
            if (has_complete_marker(b"AHCI: teste falhou:")
                    or (not args.without_disk and
                        "AHCI indisponível:".encode() in serial)):
                break
            if process.poll() is not None:
                break
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        args.log.parent.mkdir(parents=True, exist_ok=True)
        args.log.write_bytes(bytes(serial) + b"\n--- mensagens do QEMU ---\n" + bytes(errors))
        selector.close()
        if disk:
            os.unlink(disk.name)

    if not passed:
        sys.stderr.write(f"Falha no teste AHCI; log: {args.log}\n")
        sys.stderr.write(serial.decode("utf-8", errors="replace"))
        sys.stderr.write(errors.decode("utf-8", errors="replace"))
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

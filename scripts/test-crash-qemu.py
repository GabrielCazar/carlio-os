#!/usr/bin/env python3
"""Exercita panic e uma exceção x86 no QEMU com a imagem crash-test."""

import argparse
import os
import selectors
import subprocess
import sys
import time
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--iso", required=True, type=Path)
    parser.add_argument("--case", required=True,
                        choices=("panic", "fault", "watchdog", "doublefault"))
    parser.add_argument("--log", required=True, type=Path)
    parser.add_argument("--timeout", type=int, default=20)
    args = parser.parse_args()
    test_command = {"panic": "panic-teste", "fault": "fault-teste",
                    "watchdog": "hang-teste", "doublefault": "doublefault-teste"}[args.case]
    command = test_command.encode() + b"\r"
    qemu = [
        "qemu-system-x86_64", "-machine", "q35", "-accel", "tcg", "-m", "512M",
        "-cdrom", str(args.iso.resolve()), "-boot", "d", "-serial", "stdio",
        "-monitor", "none", "-display", "none", "-no-reboot", "-no-shutdown",
    ]
    process = subprocess.Popen(qemu, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, bufsize=0)
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ, "serial")
    selector.register(process.stderr, selectors.EVENT_READ, "qemu")
    serial = bytearray()
    errors = bytearray()
    sent = False
    deadline = time.monotonic() + args.timeout
    try:
        while time.monotonic() < deadline:
            for key, _ in selector.select(timeout=0.2):
                chunk = os.read(key.fileobj.fileno(), 4096)
                if not chunk:
                    continue
                if key.data == "serial":
                    serial.extend(chunk)
                    if not sent and b"carlio> " in serial:
                        process.stdin.write(command)
                        process.stdin.flush()
                        sent = True
                    data = bytes(serial)
                    if args.case == "panic":
                        passed = (b"=== CAPTURA DE PANIC ===" in data
                                  and b"panic intencional para teste" in data
                                  and b"Local:" in data
                                  and b"Captura somente serial" in data
                                  and "Modo de recuperação serial ativo; digite Enter para consultar erros.\r\n".encode() in data)
                    elif args.case == "fault":
                        passed = (b"=== CAPTURA DE EXCECAO CPU ===" in data
                                  and b"#UD" in data
                                  and b"RIP: 0x" in data
                                  and b" R15=0x" in data
                                  and "Sistema interrompido; diagnóstico serial somente leitura.\r\n".encode() in data)
                    elif args.case == "watchdog":
                        passed = (b"=== WATCHDOG: SEM PROGRESSO ===" in data
                                  and b"RIP: 0x" in data
                                  and "Sistema interrompido; diagnóstico serial somente leitura.\r\n".encode() in data)
                    else:
                        passed = (b"Vetor: 8" in data and b"#DF double fault" in data
                                  and b"RIP: 0x" in data
                                  and "Sistema interrompido; diagnóstico serial somente leitura.\r\n".encode() in data)
                    if passed:
                        return_code = 0
                        return 0
                else:
                    errors.extend(chunk)
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

    sys.stderr.write(f"Falha no teste {args.case}; log: {args.log}\n")
    sys.stderr.write(serial.decode("utf-8", errors="replace"))
    sys.stderr.write(errors.decode("utf-8", errors="replace"))
    return 1


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Exercise passive TCP through QEMU's e1000e RX/TX and a CPL3 ELF."""

import argparse
import importlib
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile

boot = importlib.import_module("test-persistence-qemu").boot


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("iso", type=Path, help="ISO built with CARLIO_DISK_EXEC_TEST=1")
    parser.add_argument("log", type=Path)
    parser.add_argument("--firmware", type=Path)
    parser.add_argument("--vars", type=Path)
    args = parser.parse_args()
    if bool(args.firmware) != bool(args.vars):
        parser.error("--firmware and --vars must be supplied together")

    with tempfile.TemporaryDirectory(prefix="carlio-tcp-") as directory:
        work = Path(directory)
        root = work / "root"
        root.mkdir()
        asm = Path(__file__).with_name("fixtures") / "tcp-echo.s"
        obj = work / "probe.o"
        subprocess.run(["as", "--64", "-o", obj, asm], check=True)
        subprocess.run(["ld", "-static", "-o", root / "carlio-disk-exec", obj], check=True)
        disk = work / "disk.raw"
        with disk.open("wb") as stream:
            stream.truncate(65 * 1024 * 1024)
        subprocess.run(["sfdisk", "--quiet", disk],
                       input=b"label: dos\nstart=2048,size=131072,type=83\n", check=True)
        subprocess.run(["mkfs.ext4", "-q", "-F", "-O",
                        "^flex_bg,^metadata_csum_seed,^orphan_file", "-E", "offset=1048576",
                        "-b", "4096", "-d", root, disk, "16384"], check=True)

        with socket.socket() as reserve:
            reserve.bind(("127.0.0.1", 0))
            port = reserve.getsockname()[1]
        qemu = [
            "qemu-system-x86_64", "-machine", "q35", "-accel", "tcg", "-cpu", "max",
            "-m", "512M", "-smp", "4", "-display", "none", "-serial", "stdio",
            "-cdrom", args.iso, "-boot", "d", "-drive", f"if=ide,format=raw,file={disk}",
            "-netdev", f"user,id=net,hostfwd=tcp:127.0.0.1:{port}-:19077",
            "-device", "e1000e,netdev=net",
        ]
        if args.firmware:
            nvram = work / "vars.fd"
            shutil.copyfile(args.vars, nvram)
            qemu += [
                "-drive", f"if=pflash,format=raw,unit=0,readonly=on,file={args.firmware}",
                "-drive", f"if=pflash,format=raw,unit=1,file={nvram}",
            ]

        def exchange():
            with socket.create_connection(("127.0.0.1", port), timeout=8) as client:
                client.sendall(b"ping!")
                client.shutdown(socket.SHUT_WR)
                with client.makefile("rb") as response:
                    assert response.read(5) == b"pong!"
                    assert response.read(1) == b""

        boot(qemu, b"", b"TCP CPL3 pronto", 90, args.log, verify=exchange)
        verified = "QEMU host: handshake, troca de bytes e FIN confirmados."
        with args.log.open("a", encoding="utf-8") as log:
            log.write(f"\n{verified}\n")
        print(f"{verified} Log: {args.log}")


if __name__ == "__main__":
    main()

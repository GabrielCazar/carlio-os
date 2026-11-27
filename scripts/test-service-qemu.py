#!/usr/bin/env python3
"""Test authenticated service supervision with an independent TCP client."""

import argparse
import os
from pathlib import Path
import re
import selectors
import shutil
import socket
import subprocess
import tempfile
import time

from provision_storage import provision


def exercise(qemu, port, password, log_path):
    with log_path.open("wb") as log, subprocess.Popen(
        qemu, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
    ) as vm, selectors.DefaultSelector() as selector:
        selector.register(vm.stdout, selectors.EVENT_READ)
        pending = bytearray()

        def send(command):
            vm.stdin.write(command + b"\r")
            vm.stdin.flush()

        def expect(pattern, timeout=8):
            deadline = time.monotonic() + timeout
            while True:
                match = re.search(pattern, pending, re.DOTALL)
                if match:
                    groups = match.groups()
                    del pending[:match.end()]
                    return groups
                if not selector.select(max(0, deadline - time.monotonic())):
                    raise TimeoutError(f"{pattern!r} ausente. Consulte {log_path}.")
                chunk = os.read(vm.stdout.fileno(), 4096)
                if not chunk:
                    raise RuntimeError(f"QEMU encerrou. Consulte {log_path}.")
                pending.extend(chunk)
                log.write(chunk)
                log.flush()

        def login(user):
            send(b"login " + user)
            expect(b"Senha: ")
            send(password)
            expect(b"sessao> ")

        def start(argument=b""):
            send(b"servico iniciar /service-tcp" + argument)
            return expect(rb"iniciado pid=(\d+)\.")[0]

        def wait_for_state(pid, state):
            deadline = time.monotonic() + 8
            expect(b"sessao> ")
            while time.monotonic() < deadline:
                send(b"servicos")
                output = expect(rb"(.*?)sessao> ")[0]
                if re.search(rb"pid=" + pid + rb" " + state, output):
                    return
                time.sleep(0.05)
            raise TimeoutError(f"PID {pid!r} sem estado {state!r}. Consulte {log_path}.")

        def stop(pid):
            send(b"servico parar " + pid)
            expect("Serviço parado".encode())

        try:
            expect(rb"probe CPL3 aguardando byte de entrada serial\.", timeout=90)
            vm.stdin.write(b"x")
            vm.stdin.flush()
            expect(b"carlio> ", timeout=90)
            login(b"operador")
            first = start()
            wait_for_state(first, b"bloqueado")
            with socket.create_connection(("127.0.0.1", port), timeout=8) as old:
                spinning = start(b" loop")
                wait_for_state(spinning, "em execução".encode())
                stop(spinning)
                old.sendall(b"A")
                assert old.recv(1) == b"A"

                send(b"sair")
                expect(b"carlio> ")
                login(b"visitante")
                send(b"servico reiniciar " + first)
                expect(rb"errno 1\)")
                send(b"sair")
                expect(b"carlio> ")
                login(b"operador")
                send(b"servicos")
                expect(rb"pid=" + first + b" bloqueado")
                send(b"servico reiniciar " + first)
                second = expect(rb"reiniciado pid=(\d+)\.")[0]
                try:
                    old.sendall(b"B")
                    assert old.recv(1) == b""
                except (BrokenPipeError, ConnectionResetError):
                    pass

            assert second != first
            wait_for_state(second, b"bloqueado")
            with socket.create_connection(("127.0.0.1", port), timeout=8) as half_closed:
                half_closed.sendall(b"!")
                assert half_closed.recv(1) == b"!"
                assert half_closed.recv(1) == b""
                half_closed.sendall(b"?")
            with socket.create_connection(("127.0.0.1", port), timeout=8) as new:
                new.sendall(b"C")
                assert new.recv(1) == b"C"
            stop(second)
            log.write(b"Host TCP: isolamento, encerramento, reabertura e parada em CPL3 confirmados.\n")
        finally:
            vm.terminate()
            try:
                vm.wait(timeout=3)
            except subprocess.TimeoutExpired:
                vm.kill()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("iso", type=Path)
    parser.add_argument("log", type=Path)
    parser.add_argument("--firmware", type=Path)
    parser.add_argument("--vars", type=Path)
    parser.add_argument("--accel", choices=("kvm", "tcg"), default="tcg")
    parser.add_argument("--scratch-disk", type=Path)
    args = parser.parse_args()
    if bool(args.firmware) != bool(args.vars):
        parser.error("--firmware e --vars são usados juntos")

    repo = Path(__file__).resolve().parents[1]
    fixtures = repo / "scripts/fixtures"
    target = (Path(os.environ.get("CARGO_TARGET_DIR", repo / "target")).resolve()
              / "debug")
    subprocess.run(["cargo", "build", "--locked", "--bin", "carlio-account",
                    "--example", "install-service-tcp",
                    "--target-dir", target.parent], cwd=repo, check=True)
    with tempfile.TemporaryDirectory(prefix="carlio-servico-") as directory:
        work = Path(directory)
        obj, elf = work / "fixture.o", work / "fixture"
        subprocess.run(["as", "--64", fixtures / "service-tcp.s", "-o", obj], check=True)
        subprocess.run(["ld", "-static", obj, "-o", elf], check=True)
        state = work / "state.raw"
        provision(state, 16, vfs=True)
        password = b"senha-descartavel-da-vm"
        for user, uid, gid, groups in (("operador", "1000", "1001", ["42"]),
                                        ("visitante", "1002", "1002", [])):
            subprocess.run([target / "carlio-account", state, user, uid, gid, *groups],
                           input=password + b"\n", check=True)
        subprocess.run([target / "examples/install-service-tcp", state, elf], check=True)

        with socket.socket() as reserve:
            reserve.bind(("127.0.0.1", 0))
            port = reserve.getsockname()[1]
        qemu = [
            "qemu-system-x86_64", "-machine", "q35", "-accel", args.accel,
            "-cpu", "host" if args.accel == "kvm" else "max", "-m", "512M",
            "-smp", "4", "-display", "none", "-serial", "stdio", "-no-reboot",
            "-cdrom", args.iso, "-boot", "d", "-drive", f"if=ide,format=raw,file={state}",
            "-netdev", f"user,id=net,hostfwd=tcp:127.0.0.1:{port}-:19077",
            "-device", "e1000e,netdev=net",
        ]
        if args.scratch_disk:
            scratch = str(args.scratch_disk.resolve()).replace(",", ",,")
            qemu += ["-drive", f"if=ide,index=1,format=qcow2,snapshot=on,file={scratch}"]
        if args.firmware:
            nvram = work / "vars.fd"
            shutil.copyfile(args.vars, nvram)
            qemu += ["-drive", f"if=pflash,format=raw,unit=0,readonly=on,file={args.firmware}",
                     "-drive", f"if=pflash,format=raw,unit=1,file={nvram}"]
        exercise(qemu, port, password, args.log)


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Testa AHCI IDENTIFY/READ/WRITE em uma imagem raw descartável do QEMU."""

import argparse
import json
import os
import selectors
import shutil
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path


class Qmp:
    hba = "/machine/peripheral/probe-hba"

    def __init__(self, channel):
        self.stream = channel.makefile("rwb")
        self.events = []
        if "QMP" not in self.receive():
            raise RuntimeError("QEMU não iniciou o canal QMP")
        self.request("qmp_capabilities")

    def receive(self):
        line = self.stream.readline()
        if not line:
            raise RuntimeError("Canal QMP encerrado")
        return json.loads(line)

    def request(self, execute, **arguments):
        self.stream.write(json.dumps({"execute": execute, "arguments": arguments}).encode() + b"\n")
        self.stream.flush()
        while True:
            response = self.receive()
            if "error" in response:
                raise RuntimeError(f"{execute}: {response['error']['desc']}")
            if "event" in response:
                self.events.append(response)
            if "return" in response:
                return response["return"]

    def hold(self, value):
        self.request("qom-set", path=self.hba, property="x-carlio-hold", value=value)

    def remove(self, disk, held):
        if not disk:
            self.request("qom-set", path=self.hba, property="realized", value=False)
            return
        self.request("device_del", id="probe-disk")
        while True:
            event = self.events.pop(0) if self.events else self.receive()
            if (event.get("event") == "DEVICE_DELETED"
                    and event.get("data", {}).get("device") == "probe-disk"):
                break
        if self.request("qom-get", path=self.hba, property="realized") is not True:
            raise RuntimeError("O controlador SATA desapareceu junto com o disco")
        if held and self.request("qom-get", path=self.hba, property="x-carlio-link") is not False:
            raise RuntimeError("O link SATA continuou ativo após a retirada")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qemu", default="qemu-system-x86_64")
    parser.add_argument("--qemu-data", type=Path)
    parser.add_argument("--firmware", type=Path)
    parser.add_argument("--iso", required=True, type=Path)
    parser.add_argument("--log", required=True, type=Path)
    parser.add_argument("--timeout", type=int, default=30)
    parser.add_argument("--accel", choices=("tcg", "kvm"), default="tcg",
                        help="acelerador do QEMU (KVM para NAS; TCG como padrão local)")
    parser.add_argument("--without-disk", action="store_true",
                        help="verifica ausência de dispositivo e não envia comandos de escrita")
    parser.add_argument("--disk-image", type=Path,
                        help="anexa uma QCOW2 existente em snapshot temporário descartável")
    removal = parser.add_mutually_exclusive_group()
    removal.add_argument("--remove-controller", action="store_true",
                        help="desconecta o controlador de teste após a submissão de uma leitura")
    removal.add_argument("--remove-disk", action="store_true",
                         help="desconecta apenas o disco SATA após a submissão de uma leitura")
    removal.add_argument("--command-timeout", action="store_true",
                         help="retém a submissão até o driver executar COMRESET")
    parser.add_argument("--hold-command", action="store_true",
                        help="retém a submissão SATA usando o QEMU com patch de diagnóstico")
    args = parser.parse_args()
    if args.hold_command and not args.remove_disk:
        parser.error("--hold-command requer --remove-disk")
    controlled_io = args.hold_command or args.command_timeout
    removal_test = args.remove_controller or args.remove_disk
    diagnostic = removal_test or args.command_timeout
    if args.without_disk and args.disk_image:
        parser.error("--without-disk não pode ser combinado com --disk-image")
    if diagnostic and (args.without_disk or args.disk_image):
        parser.error("O teste de remoção usa exclusivamente o disco RAW criado pelo teste")

    disk = None
    if args.disk_image and not args.disk_image.is_file():
        parser.error(f"imagem de disco ausente: {args.disk_image}")
    if not args.without_disk and not args.disk_image:
        # Respeita TMPDIR para permitir confinar imagens descartáveis ao sandbox da execução.
        disk = tempfile.NamedTemporaryFile(prefix="carlio-ahci-disposable-", suffix=".img", delete=False)
        disk.truncate(16 * 1024 * 1024)
        disk.close()
    qemu = [
        args.qemu, "-machine", "q35", "-accel", args.accel, "-m", "512M",
        "-smp", "4",
        "-cdrom", str(args.iso.resolve()),
        "-boot", "d", "-serial", "stdio", "-monitor", "none", "-display", "none",
        "-no-reboot", "-no-shutdown",
    ]
    if args.qemu_data:
        qemu.extend(["-L", str(args.qemu_data.resolve())])
    firmware_directory = None
    if args.firmware:
        firmware = args.firmware.resolve()
        firmware_directory = tempfile.TemporaryDirectory(prefix="carlio-ahci-uefi-")
        variables = Path(firmware_directory.name) / "vars.fd"
        shutil.copyfile(firmware.with_name(firmware.name.replace("CODE", "VARS", 1)), variables)
        qemu.extend([
            "-drive", f"if=pflash,format=raw,unit=0,readonly=on,file={firmware}",
            "-drive", f"if=pflash,format=raw,unit=1,file={variables}",
        ])
    qmp = None
    qmp_child = None
    if diagnostic:
        qmp, qmp_child = socket.socketpair()
        qmp.settimeout(5)
        qemu.extend([
            "-drive", f"file={disk.name},format=raw,if=none,id=probe,snapshot=on",
            "-device", "ich9-ahci,id=probe-hba,bus=pcie.0,addr=0x05",
            "-device", "ide-hd,id=probe-disk,bus=probe-hba.0,drive=probe",
            "-chardev", f"socket,id=qmp,fd={qmp_child.fileno()}",
            "-mon", "chardev=qmp,mode=control",
        ])
    elif args.disk_image:
        drive = f"file={args.disk_image.resolve()},format=qcow2,if=ide,index=0,snapshot=on"
        qemu[7:7] = ["-drive", drive]
    elif disk:
        drive = f"file={disk.name},format=raw,if=ide,index=0,snapshot=on"
        qemu[7:7] = ["-drive", drive]
    process = subprocess.Popen(qemu, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, bufsize=0,
                               pass_fds=(qmp_child.fileno(),) if qmp_child else ())
    if qmp_child:
        qmp_child.close()
    monitor = None
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ, "serial")
    selector.register(process.stderr, selectors.EVENT_READ, "qemu")
    serial = bytearray()
    errors = bytearray()
    sent = False
    removed = False
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
                        if controlled_io:
                            monitor = Qmp(qmp)
                            monitor.hold(True)
                            errors.extend(b"QMP: porta SATA 0 armada para reter PxCI.\n")
                        command = b"diskteste-remocao" if removal_test else b"fs-teste"
                        if args.command_timeout:
                            command = b"diskteste-timeout"
                        process.stdin.write(command + b"\r")
                        process.stdin.flush()
                        sent = True
                    if removal_test and not removed and (
                        "AHCI: leitura submetida; aguardando remoção QEMU.".encode() in serial
                    ):
                        if monitor is None:
                            monitor = Qmp(qmp)
                        monitor.remove(args.remove_disk, args.hold_command)
                        if args.hold_command:
                            errors.extend("QMP: disco removido, link SATA caiu, controlador presente.\n".encode())
                        elif args.remove_disk:
                            errors.extend("QMP: disco removido; controlador SATA presente.\n".encode())
                        process.stdin.write(b"x")
                        process.stdin.flush()
                        removed = True
                    if (args.command_timeout and not removed and
                            b"AHCI: timeout e COMRESET verificados; aguardando QEMU." in serial):
                        monitor.hold(False)
                        errors.extend("QMP: PxCI liberado após COMRESET.\n".encode())
                        process.stdin.write(b"x")
                        process.stdin.flush()
                        removed = True
                    if args.command_timeout and removed and "AHCI: nova leitura após COMRESET OK.".encode() in serial:
                        passed = True
                        return 0
                    if removed and "AHCI: remoção detectada; buffer e descritor protegidos.".encode() in serial:
                        passed = True
                        return 0
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
                    absent_line = "AHCI indisponível: dispositivo nao encontrado.\r\n".encode()
                    if (args.without_disk and absent_line in serial and
                            b"Digite 'ajuda' para ver os comandos.\r\n" in serial):
                        passed = True
                        return 0
                    if (has_complete_marker(b"AHCI: teste falhou:")
                            or (not args.without_disk and not diagnostic and "AHCI indisponível:".encode() in serial)):
                        break
                else:
                    errors.extend(chunk)
            if (has_complete_marker(b"AHCI: teste falhou:")
                    or (not args.without_disk and not diagnostic and
                        "AHCI indisponível:".encode() in serial)):
                break
            if process.poll() is not None:
                break
    except (RuntimeError, OSError, ValueError) as error:
        errors.extend(f"QMP: {error}\n".encode())
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
        if monitor:
            monitor.stream.close()
        if qmp:
            qmp.close()
        if disk:
            os.unlink(disk.name)
        if firmware_directory:
            firmware_directory.cleanup()

    if not passed:
        sys.stderr.write(f"Falha no teste AHCI; log: {args.log}\n")
        sys.stderr.write(serial.decode("utf-8", errors="replace"))
        sys.stderr.write(errors.decode("utf-8", errors="replace"))
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

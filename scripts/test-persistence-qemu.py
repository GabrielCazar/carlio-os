#!/usr/bin/env python3
"""Valida swap cifrado e persistência entre boots em imagem RAW descartável."""

import argparse
import os
import selectors
import shutil
import subprocess
import tempfile
import time
from pathlib import Path

from provision_storage import provision


def boot(qemu, command, marker, timeout, log, cut=False, swap_disabled=False):
    process = subprocess.Popen(
        qemu, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT, bufsize=0,
    )
    output = bytearray()
    sent_probe = False
    sent_command = False
    deadline = time.monotonic() + timeout
    try:
        with selectors.DefaultSelector() as selector:
            selector.register(process.stdout, selectors.EVENT_READ)
            while time.monotonic() < deadline:
                if not selector.select(timeout=0.2):
                    continue
                chunk = os.read(process.stdout.fileno(), 4096)
                if not chunk:
                    break
                output.extend(chunk)
                if not sent_probe and b"probe CPL3 aguardando byte de entrada serial." in output:
                    process.stdin.write(b"x")
                    sent_probe = True
                if not sent_command and b"carlio> " in output:
                    process.stdin.write(command + b"\r")
                    sent_command = True
                if marker in output:
                    if swap_disabled and b"Swap: reserva experimental habilitada." in output:
                        break
                    if command == b"log-teste-gravar" and "Swap VM: page fault CPL3 restaurado.".encode() not in output:
                        break
                    return
                if b"teste falhou." in output:
                    break
        raise SystemExit(f"Falha em {command.decode()}. Consulte {log}.")
    finally:
        if process.poll() is None:
            if cut:
                process.kill()
            else:
                process.terminate()
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        process.stdin.close()
        process.stdout.close()
        log.write_bytes(output)



def filesystem_fixture(image, directory, filesystem, metadata):
    volume = Path(directory) / "volume.raw"
    with volume.open("xb") as disk:
        disk.truncate(64 * 1024 * 1024)
    initial = bytes([0x35]) * 512
    if filesystem == "ext4":
        root = Path(directory) / "root"
        root.mkdir()
        (root / "probe.bin").write_bytes(initial)
        (root / "probe.bin").chmod(0o644)
        subprocess.run(["mkfs.ext4", "-q", "-F", "-O", "^flex_bg,^metadata_csum_seed,^orphan_file",
                        "-d", str(root), str(volume)], check=True)
    else:
        subprocess.run(["mkfs.fat", "-F", "32", str(volume)], check=True)
        with volume.open("r+b") as disk:
            bpb = disk.read(512)
            cluster_sectors = bpb[13]
            reserved = int.from_bytes(bpb[14:16], "little")
            fats = bpb[16]
            fat_sectors = int.from_bytes(bpb[36:40], "little")
            root_cluster = int.from_bytes(bpb[44:48], "little")
            assert root_cluster == 2
            for fat in range(fats):
                disk.seek((reserved + fat * fat_sectors) * 512 + 3 * 4)
                disk.write((0x0fffffff).to_bytes(4, "little"))
            data = reserved + fats * fat_sectors
            entry = bytearray(32)
            entry[:11] = b"PROBE   BIN"
            entry[11] = 0x20
            entry[26:28] = (3).to_bytes(2, "little")
            entry[28:32] = (512).to_bytes(4, "little")
            disk.seek(data * 512)
            disk.write(entry)
            disk.seek((data + cluster_sectors) * 512)
            disk.write(initial)
            fsinfo = int.from_bytes(bpb[48:50], "little")
            backup = int.from_bytes(bpb[50:52], "little")
            for sector in (fsinfo, backup + fsinfo):
                disk.seek(sector * 512 + 488)
                free = int.from_bytes(disk.read(4), "little")
                if free != 0xffffffff:
                    disk.seek(sector * 512 + 488)
                    disk.write((free - 1).to_bytes(4, "little"))
    with image.open("xb") as disk, volume.open("rb") as source:
        marker = b"CARLIO-FS-META1" if metadata else b"CARLIO-FS-TEST1"
        disk.write(marker + bytes([1 if filesystem == "ext4" else 2]))
        disk.seek(2048 * 512)
        shutil.copyfileobj(source, disk)


def check_filesystem_cut(image, log, filesystem, metadata):
    with image.open("rb") as disk:
        disk.seek(64 * 512)
        header = disk.read(512)
        target = int.from_bytes(header[8:16], "little")
        magic = b"CARREDO1"
        if metadata:
            magic = b"CARE4INO" if filesystem == "ext4" else b"CARF32DE"
        if header[:8] != magic or target < 2048 or target >= image.stat().st_size // 512:
            raise SystemExit(f"Journal do filesystem não encontrado. Consulte {log}.")
        redo = disk.read(512)
        disk.seek(target * 512)
        if not metadata and redo != bytes([0x6d]) * 512:
            raise SystemExit(f"Dados inesperados no journal. Consulte {log}.")
        torn = disk.read(512)
        expected = [redo[:256] + bytes(256)]
        if metadata:
            expected.append(bytes(256) + redo[256:])
        if redo == torn or torn not in expected:
            raise SystemExit(f"Setor parcial do filesystem não encontrado. Consulte {log}.")
        return target, redo


def check_metadata_recovery(image, directory, filesystem, target, redo, log):
    restored = Path(directory) / "restored.raw"
    with image.open("rb") as disk:
        disk.seek(64 * 512)
        if any(disk.read(512)):
            raise SystemExit(f"Journal continuou pendente. Consulte {log}.")
        disk.seek(target * 512)
        if disk.read(512) != redo:
            raise SystemExit(f"Setor de metadados não restaurado. Consulte {log}.")
        disk.seek(2048 * 512)
        with restored.open("xb") as output:
            shutil.copyfileobj(disk, output)
    checker = ["e2fsck", "-fn"] if filesystem == "ext4" else ["fsck.fat", "-vn"]
    with log.open("ab") as output:
        subprocess.run([*checker, str(restored)], stdout=output, stderr=subprocess.STDOUT, check=True)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--iso", required=True, type=Path)
    parser.add_argument("--firmware", type=Path)
    parser.add_argument("--log-dir", required=True, type=Path)
    scenario = parser.add_mutually_exclusive_group()
    scenario.add_argument("--filesystem", choices=["ext4", "fat32"])
    scenario.add_argument("--vfs", action="store_true")
    scenario.add_argument("--accounts", action="store_true")
    scenario.add_argument("--account-provisioner", type=Path)
    parser.add_argument("--metadata", action="store_true")
    parser.add_argument("--accel", choices=["tcg", "kvm"], default="tcg")
    parser.add_argument("--timeout", type=int, default=45)
    args = parser.parse_args()
    if args.metadata and not args.filesystem:
        parser.error("--metadata requer --filesystem")
    args.log_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="carlioos-persistence-") as directory:
        image = Path(directory) / "data.raw"
        if args.filesystem:
            filesystem_fixture(image, directory, args.filesystem, args.metadata)
        else:
            overlay = args.vfs or args.accounts or args.account_provisioner is not None
            provision(image, 16, vfs=overlay, swap_pages=0 if overlay else 1)
        qemu = [
            "qemu-system-x86_64", "-machine", "q35", "-accel", args.accel,
            "-cpu", "host" if args.accel == "kvm" else "max", "-m", "512M", "-smp", "4",
            "-cdrom", str(args.iso.resolve()), "-boot", "d",
            "-drive", f"file={image},format=raw,if=ide,index=0,cache=directsync",
            "-display", "none", "-serial", "stdio", "-monitor", "none",
            "-no-reboot", "-no-shutdown",
        ]
        if args.firmware:
            code = args.firmware.resolve()
            variables = Path(directory) / "ovmf-vars.fd"
            shutil.copyfile(code.with_name(code.name.replace("CODE", "VARS", 1)), variables)
            qemu.extend([
                "-drive", f"if=pflash,format=raw,unit=0,readonly=on,file={code}",
                "-drive", f"if=pflash,format=raw,unit=1,file={variables}",
            ])
        mode = "uefi" if args.firmware else "bios"
        if args.account_provisioner:
            subprocess.run(
                [str(args.account_provisioner.resolve()), str(image), "operador", "1000", "1000"],
                input=b"senha-descartavel-da-vm\n",
                check=True,
            )
            boot(qemu, b"", "Contas: banco protegido validado no overlay persistente.".encode(),
                 args.timeout, args.log_dir / f"bloco4-conta-provisionada-{mode}.log")
            print(f"{mode}: kernel abriu o banco provisionado fora da VM")
            return
        if args.accounts:
            for action, marker in (
                ("gravar", "Contas: banco de teste gravado no disco."),
                ("login", "Contas: sessão Argon2id emitida a partir do disco."),
            ):
                boot(qemu, f"contas-teste-{action}".encode(), marker.encode(), args.timeout,
                     args.log_dir / f"bloco4-contas-{mode}-{action}.log")
            print(f"{mode}: banco de contas persistiu e autenticou após remontagem")
            return
        if args.vfs:
            for action, marker in (
                ("gravar", "VFS: primeira geração confirmada."),
                ("ler", "VFS: dados e permissões anteriores recuperados."),
                ("alterar", "VFS: segunda geração confirmada."),
            ):
                boot(qemu, f"vfs-teste-{action}".encode(), marker.encode(), args.timeout,
                     args.log_dir / f"fase3-vfs-{mode}-{action}.log", swap_disabled=True)
            with image.open("r+b") as disk:
                # Generation 3 is bank 0. Simulate a torn commit header.
                disk.seek(4096 * 512 + 24)
                checksum = disk.read(1)
                disk.seek(-1, os.SEEK_CUR)
                disk.write(bytes([checksum[0] ^ 1]))
            boot(qemu, b"vfs-teste-ler", "VFS: dados e permissões anteriores recuperados.".encode(),
                 args.timeout, args.log_dir / f"fase3-vfs-{mode}-recuperar.log")
            print(f"{mode}: overlay VFS recuperou dados e metadados")
            return
        if args.filesystem:
            label = f"fase3-{args.filesystem}-{mode}" + ("-meta" if args.metadata else "")
            command = b"fs-meta" if args.metadata else b"fs-teste"
            cut_log = args.log_dir / f"{label}-corte.log"
            boot(qemu, command + b"-corte", "Armazenamento: setor parcial escrito; flush pendente.".encode(),
                 args.timeout, cut_log, cut=True)
            target, redo = check_filesystem_cut(image, cut_log, args.filesystem, args.metadata)
            recovery_log = args.log_dir / f"{label}-recuperacao.log"
            success = ("Filesystem: metadados nativos recuperados e segunda recuperação idempotente OK."
                       if args.metadata else "Filesystem: redo após corte e segunda recuperação idempotente OK.")
            boot(qemu, command + b"-recuperar", success.encode(), args.timeout, recovery_log)
            if args.metadata:
                check_metadata_recovery(image, directory, args.filesystem, target, redo, recovery_log)
            print(f"{mode}: {args.filesystem} recuperou a escrita após corte")
            return
        for step, marker in (
            ("gravar", "Log persistente: gravação e flush OK."),
            ("corte", "Armazenamento: setor parcial escrito; flush pendente."),
            ("ler", "Log persistente: leitura após reboot OK."),
        ):
            log = args.log_dir / f"fase3-persistencia-{mode}-{step}.log"
            boot(qemu, f"log-teste-{step}".encode(), marker.encode() + b"\r\n",
                 args.timeout, log, cut=step == "corte")
            if step == "gravar":
                with image.open("rb") as disk:
                    disk.seek(4096 * 512)
                    ciphertext = disk.read(4096)
                if ciphertext in (bytes(4096), bytes([0x6a]) * 4096):
                    raise SystemExit(f"Página cifrada não encontrada na imagem. Consulte {log}.")
            if step == "corte":
                with image.open("rb") as disk:
                    disk.seek(2049 * 512)
                    sector = disk.read(512)
                if sector[:16] != b"CARLOG01" + (5).to_bytes(8, "little") or any(sector[256:]):
                    raise SystemExit(f"Escrita parcial não encontrada na imagem. Consulte {log}.")
        print(f"{mode}: chave de boot, swap cifrado, coletor e recuperação de setor parcial verificados")


if __name__ == "__main__":
    main()

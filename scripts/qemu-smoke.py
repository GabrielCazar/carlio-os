#!/usr/bin/env python3
"""Teste automatizado do terminal serial do CarlioOS em uma VM QEMU."""

import argparse
import os
import selectors
import shutil
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path


def snapshot_drive(image: Path, image_format: str, overlay: Path) -> str:
    """Keep guest writes in a qcow2 overlay with an explicit temporary location."""
    subprocess.run(
        ["qemu-img", "create", "-q", "-f", "qcow2", "-F", image_format,
         "-b", str(image.resolve()), str(overlay)],
        check=True,
    )
    drive_path = str(overlay).replace(",", ",,")
    return f"if=ide,format=qcow2,file={drive_path}"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    boot = parser.add_mutually_exclusive_group(required=True)
    boot.add_argument("--iso", type=Path)
    boot.add_argument("--boot-image", type=Path, help="disco raw inicializável, em snapshot")
    parser.add_argument("--accel", default="tcg", choices=("tcg", "kvm"))
    parser.add_argument("--firmware", type=Path, help="Imagem OVMF para testar UEFI")
    parser.add_argument("--log", required=True, type=Path)
    parser.add_argument("--timeout", type=int, default=25)
    parser.add_argument("--cpus", type=int, default=4, help="CPUs virtuais para o teste SMP")
    parser.add_argument("--smp-cycles", type=int, help="Ciclos mmap/munmap esperados em cada AP de teste")
    parser.add_argument("--expect-runtime-stress", action="store_true",
                        help="exige heap concorrente, faults isolados e watermark das duas entradas")
    parser.add_argument("--memory", default="512M", help="RAM guest QEMU (ex.: 5G, 32G)")
    parser.add_argument("--expect-high-frame", action="store_true",
                        help="exige alocação, escrita, leitura e liberação acima de 4 GiB")
    parser.add_argument("--herm-image", type=Path, help="imagem HermFS descartável anexada ao AHCI")
    parser.add_argument("--disk-image", type=Path,
                        help="imagem de disco anexada ao AHCI; escrita fica em snapshot temporário")
    parser.add_argument("--expect-disk-exec", action="store_true",
                        help="exige saída do ELF CPL3 carregado do disco por execve")
    parser.add_argument("--expect-linux-uname", action="store_true",
                        help="exige que um uname com glibc escreva Linux no serial")
    parser.add_argument("--expect-openssh", nargs="?", const="version", choices=("version", "config", "listen", "banner"),
                        help="exige a versão, a configuração efetiva ou a abertura do listener ou banner TCP do sshd")
    parser.add_argument("--expect-package-version", choices=("smbd", "pacman"),
                        help="exige saída de versão e status zero em CPL3")
    parser.add_argument("--expect-pegasus", action="store_true",
                        help="exige que pegasusd conclua as operações locais de diretório e saia com status zero")
    parser.add_argument("--expect-lockdown", choices=("none", "integrity", "confidentiality"),
                        help="exige o nível de lockdown no log de boot")
    parser.add_argument("--expect-physical-report", action="store_true",
                        help="exige diagnóstico automático sem entrada serial durante o boot")
    parser.add_argument("--expect-disk-read", action="store_true",
                        help="exige montagem e leitura CPL3 de /probe.bin com disk-read-test")
    parser.add_argument("--expect-rootfs", choices=("initialized", "mounted"),
                        help="exige mensagem positiva de inicialização ou remontagem do rootfs")
    parser.add_argument("--qemu-debug-log", type=Path,
                        help="captura eventos internos do QEMU para diagnóstico")
    args = parser.parse_args()
    if not 1 <= args.cpus <= 64:
        parser.error("--cpus deve estar entre 1 e 64")
    if args.smp_cycles is not None and args.smp_cycles < 1:
        parser.error("--smp-cycles deve ser positivo")
    if args.herm_image and args.disk_image:
        parser.error("use apenas um entre --herm-image e --disk-image")
    if args.expect_disk_exec and not args.disk_image:
        parser.error("--expect-disk-exec exige --disk-image")
    if args.expect_linux_uname and not args.disk_image:
        parser.error("--expect-linux-uname exige --disk-image")
    if args.expect_openssh and not args.disk_image:
        parser.error("--expect-openssh exige --disk-image")
    if (args.expect_pegasus or args.expect_package_version) and not args.disk_image:
        parser.error("provas de Pegasus DS e pacotes exigem --disk-image")
    if args.expect_pegasus and (args.expect_disk_exec or args.expect_linux_uname or args.expect_disk_read):
        parser.error("--expect-pegasus é um modo de execução de disco independente")
    if args.expect_disk_read and not args.disk_image:
        parser.error("--expect-disk-read exige --disk-image")
    if args.expect_rootfs and not args.herm_image:
        parser.error("--expect-rootfs exige --herm-image")

    network = "user,id=net0"
    if args.expect_openssh == "banner":
        with socket.socket() as reserved:
            reserved.bind(("127.0.0.1", 0))
            banner_port = reserved.getsockname()[1]
        network += f",hostfwd=tcp:127.0.0.1:{banner_port}-:2222"

    command = [
        "qemu-system-x86_64",
        "-machine", "q35",
        "-accel", args.accel,
        "-m", args.memory,
        "-smp", str(args.cpus),
        "-nic", "none",
        "-netdev", network,
        "-device", "e1000e,netdev=net0",
        "-serial", "stdio",
        "-monitor", "none",
        "-display", "none",
        "-no-reboot",
        "-no-shutdown",
    ]
    if args.expect_linux_uname or args.expect_pegasus or args.expect_openssh or args.expect_package_version:
        command.extend(("-cpu", "host" if args.accel == "kvm" else "max"))
    snapshots = None
    if args.boot_image or args.disk_image:
        args.log.parent.mkdir(parents=True, exist_ok=True)
        snapshots = tempfile.TemporaryDirectory(prefix="carlio-qemu-", dir=args.log.parent)
    if args.iso:
        command.extend(("-cdrom", str(args.iso.resolve()), "-boot", "d"))
    else:
        boot_drive = snapshot_drive(args.boot_image, "raw", Path(snapshots.name) / "boot.qcow2")
        command.extend(("-drive", boot_drive, "-boot", "c"))
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
    if args.herm_image:
        image_path = args.herm_image.resolve()
        if not image_path.is_file():
            parser.error(f"imagem Hermione ausente: {image_path}")
        command.extend(("-drive", f"if=ide,format=raw,file={image_path}"))
    if args.disk_image:
        image_path = args.disk_image.resolve()
        if not image_path.is_file():
            parser.error(f"imagem de disco ausente: {image_path}")
        data_drive = snapshot_drive(image_path, "qcow2", Path(snapshots.name) / "data.qcow2")
        command.extend(("-drive", data_drive))
    if args.qemu_debug_log:
        args.qemu_debug_log.parent.mkdir(parents=True, exist_ok=True)
        command.extend(("-d", "int", "-D", str(args.qemu_debug_log.resolve())))

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
    serial_probe_sent = False
    checks = (
        (b"Digite 'ajuda'", b"ajuda\r"),
        ("Comandos: ajuda, sobre, usuario, permissoes, erros, logs, limpar, parar.".encode(), b"diagnostico\r"),
        (b"Tempo desde o boot (s):", b"erros\r"),
        (b"Erros registrados:", b"logs\r"),
        (b"# Eventos do kernel (buffer vol\xc3\xa1til)", b"sobre\r"),
        ("carlIO OS | Criado por João G.R.Santos".encode(), None),
        ("Sistema operacional experimental x86_64 escrito em Rust.".encode(), b"parar\r"),
        ("Processador parado.".encode(), None),
    )
    application_result = None
    if args.expect_pegasus:
        application_result = "Pegasus DS: operações locais de diretório verificadas.".encode()
    elif args.expect_package_version:
        application_result = {"smbd": b"Version ", "pacman": b"Pacman v"}[args.expect_package_version]
    if application_result:
        checks = (
            (application_result, None),
            (b"Runtime: prova de execve encerrou com status=0.", None),
        )
    elif args.expect_openssh in ("listen", "banner"):
        checks = ((b"Server listening on 0.0.0.0 port 2222.", None),)
    application_offset = 0
    deadline = time.monotonic() + args.timeout
    banner = b""

    try:
        while stage < len(checks) and time.monotonic() < deadline:
            for key, _ in selector.select(timeout=0.25):
                if stage == len(checks):
                    break
                chunk = os.read(key.fileobj.fileno(), 4096)
                if not chunk:
                    continue
                if key.data == "serial":
                    output.extend(chunk)
                    haystack = bytes(output)
                    if (not serial_probe_sent
                            and b"ABI Linux: probe CPL3 aguardando byte de entrada serial." in haystack):
                        process.stdin.write(b"Q")
                        process.stdin.flush()
                        serial_probe_sent = True
                else:
                    errors.extend(chunk)
                    haystack = bytes(errors)
                if application_result:
                    if key.data == "serial":
                        while stage < len(checks):
                            expected, _ = checks[stage]
                            position = haystack.find(expected, application_offset)
                            if position < 0:
                                break
                            application_offset = position + len(expected)
                            stage += 1
                    continue
                expected, response = checks[stage]
                if key.data == "serial" and expected in haystack:
                    if response:
                        process.stdin.write(response)
                        process.stdin.flush()
                    stage += 1
            if process.poll() is not None and stage < len(checks):
                break
        if args.expect_openssh == "banner" and stage == len(checks):
            try:
                with socket.create_connection(("127.0.0.1", banner_port), timeout=5) as client:
                    client.setblocking(False)
                    selector.register(client, selectors.EVENT_READ, "client")
                    while len(banner) < 256 and b"\n" not in banner:
                        remaining = deadline - time.monotonic()
                        if remaining <= 0:
                            break
                        for key, _ in selector.select(timeout=min(0.25, remaining)):
                            chunk = os.read(key.fileobj.fileno(), 4096)
                            if not chunk:
                                selector.unregister(key.fileobj)
                                if key.data == "client":
                                    remaining = 0
                                    break
                                continue
                            if key.data == "client":
                                banner += chunk[:256 - len(banner)]
                            else:
                                (output if key.data == "serial" else errors).extend(chunk)
                        if remaining == 0 or process.poll() is not None:
                            break
                    banner = banner.splitlines(keepends=True)[0] if banner else b""
            except OSError as error:
                errors.extend(f"Falha na leitura do banner SSH: {error}\n".encode())
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
        if snapshots:
            snapshots.cleanup()

    if stage != len(checks):
        sys.stderr.write(f"Falha no teste QEMU: etapa {stage + 1}/{len(checks)}. Log: {args.log}\n")
        sys.stderr.write(output.decode("utf-8", errors="replace"))
        sys.stderr.write(errors.decode("utf-8", errors="replace"))
        return 1

    if args.expect_disk_exec and (
            b"execve: ELF do disco executado em CPL3" not in output
            or b"execve: SSE iniciado e preservado em CPL3" not in output
            or b"ABI Linux: stat, mutacoes e fsync verificados em CPL3" not in output
            or b"ABI Linux: permissoes VFS e ancestrais verificados em CPL3" not in output
            or b"ABI Linux: socket IPv4 e porta efemera verificados em CPL3" not in output
            or b"Runtime: prova de execve encerrou com status=0." not in output):
        sys.stderr.write(f"Validação de ELF, SSE, VFS ou sockets ausente em CPL3. Log: {args.log}\n")
        return 1

    if args.expect_linux_uname and (
            b"Linux" not in output.splitlines()
            or b"Runtime: prova de execve encerrou com status=0." not in output):
        sys.stderr.write(f"Saída ou status zero do uname Linux ausente. Log: {args.log}\n")
        return 1

    if args.expect_pegasus:
        print(f"Operações locais do Pegasus DS em CPL3 concluídas com status zero. Log: {args.log}")
        return 0

    if args.expect_package_version:
        print(f"{args.expect_package_version} executou --version em CPL3 com status zero. Log: {args.log}")
        return 0

    if args.expect_openssh == "banner":
        if not banner.startswith(b"SSH-2.0-OpenSSH_") or not banner.endswith(b"\r\n"):
            sys.stderr.write(f"Banner OpenSSH ausente ou incompleto. Log: {args.log}\n")
            return 1
        print(f"Cliente TCP recebeu {banner.decode('ascii', errors='replace').strip()}. Log: {args.log}")
        return 0

    if args.expect_openssh == "listen":
        print(f"OpenSSH abriu o listener IPv4 na porta 2222. Log: {args.log}")
        return 0

    if args.expect_openssh:
        lines = bytes(output).splitlines()
        if args.expect_openssh == "version":
            expected_output = any(line.startswith(b"OpenSSH_") for line in lines)
        else:
            effective = {line.lower() for line in lines}
            expected_output = {b"port 2222", b"hostkey /etc/ssh/probe_ed25519"} <= effective
        if not expected_output or b"Runtime: prova de execve encerrou com status=0." not in output:
            sys.stderr.write(f"Saída ou status zero de sshd ausente. Log: {args.log}\n")
            return 1

    if args.expect_disk_read and (
            "VFS: rootfs montado em / para leitura".encode() not in output
            or "ABI Linux: openat/read/close CPL3 leu arquivo pelo VFS.".encode() not in output):
        sys.stderr.write(f"Leitura CPL3 do arquivo no disco ausente. Log: {args.log}\n")
        return 1

    if args.expect_rootfs:
        marker = (b"Hermione rootfs inicial persistido" if args.expect_rootfs == "initialized"
                  else b"Hermione rootfs persistente montado")
        if marker not in output:
            sys.stderr.write(f"Rootfs não confirmou estado {args.expect_rootfs}. Log: {args.log}\n")
            sys.stderr.write(output.decode("utf-8", errors="replace"))
            return 1

    expected_signatures = (
        "Pilhas do kernel: páginas de guarda instaladas=517.".encode(),
        "IRQ stack: pilha por CPU e duas fatias aninhadas passaram.".encode(),
        b"SATA/AHCI", b" v\xc3\xaddeo", b" rede",
        b"NIC Intel 82574L: filas DMA TX/RX ativas",
        b"DHCP IPv4: concessao recebida",
        b"ARP IPv4: gateway respondeu",
        b"Ring 3: GDT, TSS, RSP0 e seletores de usu\xc3\xa1rio configurados; CPL continua 0.",
        "Framebuffer Limine: RGB ativo; renderização de pixels validada.".encode(),
        b"Framebuffer de firmware: ativo",
        "Criado por João G.R.Santos".encode(),
        "Login de teste: senha incorreta negada e sessão Argon2id emitida.".encode(),
        "ABI Linux: gettid e clock_gettime monotônico CPL3 passaram.".encode(),
        "Segurança CPL3: openat negou arquivo 0600 de outro UID com EACCES.".encode(),
        "ABI Linux: mmap/mprotect/munmap/brk CPL3 validaram ownership e permissões de página.".encode(),
        "SMP CPL3: mmap/munmap concluídos nos dois APs; sobreposição simultânea e retorno a idle confirmados.".encode(),
        "Reaper SMP: BSP e APs confirmaram CR3 kernel; ACKs dos APs=".encode(),
        "Allocator por processo: frames livres restaurados ao baseline=".encode(),
        "; CPL3 ativo=0; stacks, VM e page tables liberáveis.".encode(),
        "; alocação, escrita, leitura e liberação passaram.".encode(),
        "Ring 3 rotinas: ELF carregado em CPL3, scheduler e preempção entre dois CR3 passaram; pilhas recuperadas pelo reaper.".encode(),
        "Ring 3: probe CPL3, retorno seguro e fault #UD isolado passaram.".encode(),
        "TLB SMP: shootdowns enviados=".encode(),
        "Heap kernel: regiões dinâmicas, alinhamento, escrita/leitura, expansão e liberação passaram.".encode(),
        f"SMP Limine: CPUs detectadas={args.cpus}, APs iniciadas={args.cpus - 1}, aguardadas={args.cpus - 1}; APs aguardam despacho de tarefa, scheduler ainda desativado.".encode(),
        f"SMP trabalho: CPUs AP={args.cpus - 1}, tarefas concluídas={args.cpus - 1}, resultados válidos={args.cpus - 1}; despacho de prova concluído.".encode(),
    )
    # Each unmodified probe owns two user pages and four page-table pages.
    # The disk workload replaces the first probe's address space.
    memory_probe = b"2/4/6/16384/65537"
    memory_prefix = "Memória user por processo (folhas/tabelas/total/limite-folhas/limite-total): ".encode()
    if args.expect_disk_exec or args.expect_linux_uname or args.expect_openssh:
        memory_signature = b" " + b" ".join((memory_probe,) * 2)
    else:
        memory_signature = memory_prefix + b" ".join((memory_probe,) * 3)
    expected_signatures += (memory_signature + b".",)
    if args.expect_physical_report:
        expected_signatures += (
            "ABI Linux: teste de entrada serial omitido no boot autônomo.".encode(),
            b"CARLIO-PHYSICAL-REPORT BEGIN",
            b"CARLIO-PHYSICAL-REPORT END",
            b"Log persistente: inativo",
        )
        if serial_probe_sent:
            sys.stderr.write("O perfil físico solicitou entrada serial durante o boot.\n")
            return 1
    else:
        expected_signatures += (b"ABI Linux: read(0) CPL3 recebeu entrada serial.",)
    if args.expect_runtime_stress:
        expected_signatures += (
            "Heap global: alocação Rust maior que a arena bootstrap passou.".encode(),
            b"SMP CPL3: heap global concorrente e dois page faults isolados confirmados.",
            "Pilha kernel observada após SYSCALL".encode(),
            "Pilha kernel observada após int 0x80".encode(),
        )
    if args.expect_high_frame:
        expected_signatures += (
            ", frame acima de 4 GiB alocado e testado; alocação, escrita, leitura e liberação passaram.".encode(),
        )
    if args.smp_cycles is not None:
        expected_signatures += (
            f"SMP CPL3 ciclos: AP1={args.smp_cycles}, AP2={args.smp_cycles}.\r\n".encode(),
        )
    if args.expect_lockdown:
        expected_signatures += (f"Lockdown: {args.expect_lockdown}\r\n".encode(),)
    missing_signatures = [signature.decode() for signature in expected_signatures if signature not in output]
    if missing_signatures:
        sys.stderr.write(f"Marcadores ausentes no smoke de boot. Log: {args.log}\n")
        for signature in missing_signatures:
            sys.stderr.write(f"- {signature}\n")
        sys.stderr.write(output.decode("utf-8", errors="replace"))
        return 1


    print(output.decode("utf-8", errors="replace"), end="")
    print(f"Teste QEMU aprovado ({args.accel}); log: {args.log}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

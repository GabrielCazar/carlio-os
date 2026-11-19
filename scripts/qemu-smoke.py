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
    parser.add_argument("--cpus", type=int, default=4, help="CPUs virtuais para o teste SMP")
    parser.add_argument("--memory", default="512M", help="RAM guest QEMU (ex.: 5G, 32G)")
    parser.add_argument("--herm-image", type=Path, help="imagem HermFS descartável anexada ao AHCI")
    parser.add_argument("--expect-rootfs", choices=("initialized", "mounted"),
                        help="exige mensagem positiva de inicialização ou remontagem do rootfs")
    parser.add_argument("--qemu-debug-log", type=Path,
                        help="captura eventos internos do QEMU para diagnóstico")
    args = parser.parse_args()
    if not 1 <= args.cpus <= 64:
        parser.error("--cpus deve estar entre 1 e 64")
    if args.expect_rootfs and not args.herm_image:
        parser.error("--expect-rootfs exige --herm-image")

    command = [
        "qemu-system-x86_64",
        "-machine", "q35",
        "-accel", args.accel,
        "-m", args.memory,
        "-smp", str(args.cpus),
        "-nic", "none",
        "-netdev", "user,id=net0",
        "-device", "e1000e,netdev=net0",
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
    if args.herm_image:
        image_path = args.herm_image.resolve()
        if not image_path.is_file():
            parser.error(f"imagem Hermione ausente: {image_path}")
        command.extend(("-drive", f"if=ide,format=raw,file={image_path}"))
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
        ("Comandos: ajuda, sobre, usuario, permissoes, erros, logs, limpar, parar.".encode(), b"erros\r"),
        (b"Erros registrados:", b"logs\r"),
        (b"# Eventos do kernel (buffer vol\xc3\xa1til)", b"sobre\r"),
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
                    if (not serial_probe_sent
                            and b"ABI Linux: probe CPL3 aguardando byte de entrada serial." in haystack):
                        process.stdin.write(b"Q")
                        process.stdin.flush()
                        serial_probe_sent = True
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

    if args.expect_rootfs:
        marker = (b"Hermione rootfs inicial persistido" if args.expect_rootfs == "initialized"
                  else b"Hermione rootfs persistente montado")
        if marker not in output:
            sys.stderr.write(f"Rootfs não confirmou estado {args.expect_rootfs}. Log: {args.log}\n")
            sys.stderr.write(output.decode("utf-8", errors="replace"))
            return 1

    if any(signature not in output for signature in (
        b"SATA/AHCI", b" v\xc3\xaddeo", b" rede",
        b"NIC Intel 82574L: filas DMA TX/RX ativas",
        b"DHCP IPv4: concessao recebida",
        b"Ring 3: GDT, TSS, RSP0 e seletores de usu\xc3\xa1rio configurados; CPL continua 0.",
        "Framebuffer Limine: RGB ativo; renderização de pixels validada.".encode(),
        "Criado por João G.R.Santos".encode(),
        b"ABI Linux: read(0) CPL3 recebeu entrada serial.",
        "ABI Linux: gettid e clock_gettime monotônico CPL3 passaram.".encode(),
        "Segurança CPL3: openat negou arquivo 0600 de outro UID com EACCES.".encode(),
        "ABI Linux: mmap/mprotect/munmap/brk CPL3 validaram ownership e permissões de página.".encode(),
        "SMP CPL3: mmap/munmap concluídos nos dois APs; sobreposição simultânea e retorno a idle confirmados.".encode(),
        "Memória user por processo (folhas/tabelas/total/limite-folhas/limite-total): 2/4/6/128/513 2/4/6/128/513 2/4/6/128/513.".encode(),
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
    )):
        sys.stderr.write(f"Inventario PCI ou tela inicial incompletos. Log: {args.log}\n")
        sys.stderr.write(output.decode("utf-8", errors="replace"))
        return 1


    print(output.decode("utf-8", errors="replace"), end="")
    print(f"Teste QEMU aprovado ({args.accel}); log: {args.log}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Smoke-test an installed SSH/SFTP and SMB deployment with stock clients."""

import argparse
from contextlib import suppress
from pathlib import Path
import subprocess
import tempfile
import uuid


def run(args, directory, *, batch=None):
    return subprocess.run(
        args,
        cwd=directory,
        input=batch,
        text=True,
        stdout=subprocess.PIPE,
        check=True,
        timeout=30,
    ).stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ssh-target", required=True, help="usuário@host ou alias do OpenSSH")
    parser.add_argument("--ssh-port", type=int, default=22)
    parser.add_argument("--known-hosts", required=True, type=Path)
    parser.add_argument("--uid", required=True, type=int)
    parser.add_argument("--smb-share", required=True, help="//host/compartilhamento de teste")
    parser.add_argument("--smb-port", type=int, default=445)
    parser.add_argument("--smb-auth-file", required=True, type=Path)
    args = parser.parse_args()

    ssh_options = [
        "-oBatchMode=yes",
        "-oStrictHostKeyChecking=yes",
        "-oGlobalKnownHostsFile=none",
        f"-oUserKnownHostsFile={args.known_hosts.resolve(strict=True)}",
    ]
    remote = f"carlio-check-{uuid.uuid4().hex}"
    with tempfile.TemporaryDirectory(prefix="carlio-file-services-") as tmp:
        directory = Path(tmp)
        (directory / "source").write_bytes(b"CarlioOS\x00\xff\n")
        uid = run(
            ["ssh", *ssh_options, "-p", str(args.ssh_port), "--", args.ssh_target, "id -u"],
            directory,
        )
        if uid.strip() != str(args.uid):
            raise ValueError(f"O comando SSH retornou UID {uid.strip()!r}, esperado {args.uid}")
        sftp = ["sftp", *ssh_options, "-P", str(args.ssh_port), "-b", "-", "--", args.ssh_target]
        try:
            run(sftp, directory, batch=f"put source {remote}\nget {remote} sftp-result\nrm {remote}\n")
        except (subprocess.SubprocessError, OSError):
            with suppress(subprocess.SubprocessError, OSError):
                run(sftp, directory, batch=f"-rm {remote}\n")
            raise
        if (directory / "sftp-result").read_bytes() != (directory / "source").read_bytes():
            raise ValueError("Conteúdo recebido via SFTP difere do original")
        smb = [
            "smbclient", args.smb_share, "-p", str(args.smb_port),
            "--option=client min protocol=SMB2",
            "-A", str(args.smb_auth_file.resolve(strict=True)),
        ]
        try:
            run([*smb, "-c", f"put source {remote}"], directory)
            run([*smb, "-c", f"get {remote} smb-result"], directory)
            run([*smb, "-c", f"del {remote}"], directory)
        except (subprocess.SubprocessError, OSError):
            with suppress(subprocess.SubprocessError, OSError):
                run([*smb, "-c", f"del {remote}"], directory)
            raise
        if (directory / "smb-result").read_bytes() != (directory / "source").read_bytes():
            raise ValueError("Conteúdo recebido via SMB difere do original")
    print("SSH: UID confirmado. SFTP e SMB: transferência binária confirmada.")


if __name__ == "__main__":
    main()

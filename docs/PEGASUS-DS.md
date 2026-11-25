# Integração com Pegasus DS

O teste compila o `pegasusd` do checkout vizinho com a feature
`directory-diagnostic`, instala o ELF e as bibliotecas glibc do host numa
imagem ext4 descartável e usa `execve` para executá-lo em CPL3. O diagnóstico
passa pelo Core e pelo adaptador LDAP tipado, com inclusão, consulta,
comparação, alteração e exclusão em memória. Também verifica a recusa
anônima e confirma que a falha da auditoria bloqueia uma alteração.
O QEMU exige a confirmação dessas operações no serial seguida de saída zero.

Em um host Linux x86-64 com glibc e as ferramentas de build e QEMU do
CarlioOS, execute na raiz do repositório:

```bash
CARLIO_DISK_EXEC_TEST=1 bash scripts/build-iso.sh
TMPDIR="$PWD/target" CARLIO_TEST_PEGASUS_ROOT=../pegasus_ds \
  bash scripts/test-disk-exec-qemu.sh build/carlio-os.iso target/pegasus-bios.log --timeout 120
TMPDIR="$PWD/target" CARLIO_TEST_PEGASUS_ROOT=../pegasus_ds \
  bash scripts/test-disk-exec-qemu.sh build/carlio-os.iso target/pegasus-uefi.log \
  --timeout 180 --firmware /usr/share/edk2/x64/OVMF_CODE.4m.fd
```

O teste usa Rust stable para o Pegasus DS e nightly para o kernel. A imagem
ext4 é criada para cada execução. O log serial fica no caminho passado ao
script. Opções do QEMU, como `--firmware`, podem ser passadas após o log.

A inicialização glibc/Rust passou a usar `poll` sem espera nos descritores
padrão, `rt_sigaction` para a disposição de SIGPIPE e `getrandom` com
entropia de hardware. O kernel fornece metadados do console por descritor
e contabiliza até 4096 páginas de usuário por processo (16 MiB com páginas
de 4 KiB), incluindo ELF, loader, bibliotecas, pilha e heap.

O diagnóstico passou em BIOS e UEFI no QEMU local (TCG) e no KVM do NAS,
com quatro CPUs por VM.

O diagnóstico exercita o diretório em memória e usa identidades sintéticas.
O serviço de rede requer autenticação e armazenamento. TLS/Kerberos,
persistência recuperável, NSS/PAM e dircache continuam pendentes. Consulte o
[contrato da ABI](KERNEL_ABI.md) e o [protocolo de dircache](DIRCACHE_PROTOCOL.md).

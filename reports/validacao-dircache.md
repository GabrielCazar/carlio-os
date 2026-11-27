# Validação do dircache e da supervisão de serviços

## Broker e isolamento em CPL3

A prova usa um backend de teste estático assinado e um cliente independente.
O backend confere que roda com UID 1000. O console recusa o caminho alternativo
da tabela com `EPERM`. O cliente sem autorização de registro recebe `EACCES`
no bind e obtém o registro POSIX esperado pelo socket `AF_UNIX SOCK_SEQPACKET`.
O console encerra o backend enquanto ele repete `accept4`.

O cenário passou em BIOS e UEFI com quatro vCPUs nos dois aceleradores. A
prova do NAS usou `nas-manjaro`, uma ISO de diagnóstico e uma imagem de estado
descartável em `/tmp/carlioos-dircache.*`. Nenhum disco físico foi passado ao
QEMU.

| Ambiente | BIOS | UEFI |
| --- | --- | --- |
| QEMU/TCG local | [Aprovado](logs/dircache-supervisionado-bios.log) | [Aprovado](logs/dircache-supervisionado-uefi.log) |
| QEMU/KVM no NAS | [Aprovado](logs/dircache-supervisionado-nas-bios.log) | [Aprovado](logs/dircache-supervisionado-nas-uefi.log) |
| ISO padrão no QEMU/TCG | [Boot aprovado](logs/revisao-local-bios.log) | [Boot aprovado](logs/revisao-local-uefi.log) |

O teste `scripts/test-service-qemu.py` também passou em BIOS e UEFI no QEMU
local. Um cliente TCP conectado ao loopback verificou continuidade enquanto
outro serviço era parado, recusa de reinício por outra conta, fechamento da
conexão anterior e reabertura após reinício. [BIOS](logs/revisao-servicos-bios.log) ·
[UEFI](logs/revisao-servicos-uefi.log)

## Revisão no host

- `cargo test --offline --all-targets` passou com 269 testes e um ignorado.
  [Log CarlioOS](logs/revisao-host-carlioos.log)
- `cargo +stable test --offline --locked --workspace --all-features` passou
  com 16 testes no Pegasus DS. Inclui o checkpoint de diagnóstico e o
  adaptador SEQPACKET testado no host. [Log Pegasus DS](logs/revisao-host-pegasus.log)
- `cargo fmt --check` encontrou diferenças em 14 arquivos da árvore de
  trabalho. `cargo clippy --offline --all-targets -- -D warnings` reprovou
  por duas lints de `src/pipe.rs`. Por isso o boot da ISO padrão foi
  executado diretamente com `scripts/build-iso.sh` e
  `scripts/qemu-smoke.py`. [Formatação](logs/revisao-host-fmt.txt) ·
  [Clippy](logs/revisao-host-clippy.txt)

## Validação do pegasusd no perfil de serviço

A ISO incorporou o ELF estático `ET_EXEC` de `pegasusd --dircache`, compilado
a partir da árvore corrente do Pegasus DS, baseada em `51d72ef`. O
SHA-256 do ELF de teste foi
`89f7f23965a99cfca910eb12ce32871b7feb782e7519fb93a799e43ebb81f5bd`.
O disco descartável forneceu a conta UID 1000 e a tabela de leitura
`/etc/pegasusds/identities.tsv`.

| Ambiente | Firmware | Resultado com 4 vCPUs |
| --- | --- | --- |
| Host, QEMU TCG | BIOS e UEFI | 6 consultas e 2 ciclos de início e parada por firmware |
| NAS Manjaro, QEMU KVM | BIOS e UEFI | 6 consultas e 2 ciclos de início e parada por firmware |

Em cada VM, o console recusou o lançamento em primeiro plano e o
argumento com caminho de tabela alternativo. O cliente CPL3 tentou
registrar o endpoint, recebeu `EACCES` e depois consultou o daemon,
obtendo `alice	1001	1002` e status zero. A parada liberou o endpoint
para o próximo ciclo. A suíte do kernel com `--features dircache`
passou com 261 testes, incluindo recusas de acesso a outro caminho e
de syscalls proibidas no perfil.

Os testes usaram a árvore em desenvolvimento e uma chave efêmera.
O perfil usa as cotas comuns de memória e o escalonador compartilhado.
Ficam em aberto a homologação para distribuição, o listener LDAP com
TLS, a persistência transacional do diretório e NSS/PAM. Consulte os
[diagramas do perfil de serviço](../docs/PEGASUSD-ISOLAMENTO.md).

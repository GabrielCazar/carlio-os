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

## Critérios em aberto

O teste do kernel cobre a fronteira de registro, consulta e parada com um
backend sintético. A homologação da execução do `pegasusd` no perfil de
serviço, o isolamento completo, o listener LDAP com TLS, a persistência
transacional e NSS/PAM continuam pendentes. O ELF estático do daemon excede
o limite de arquivo do overlay testado. A VM com a reserva persistente no
primeiro disco AHCI não montou o ext4 do segundo disco.

Os resultados foram obtidos na árvore de trabalho corrente, com mudanças
preexistentes de várias áreas. O código de lançamento supervisionado
depende da supervisão ainda em desenvolvimento nessa árvore. As evidências
registram a configuração exercitada e não homologam uma revisão de distribuição.

# Relatório — Fase 5: ABI para pegasusDS

## Contrato inicial — 2026-11-18

Congelado documentalmente o contrato `KERNEL_ABI.md` v1.0.0 com a tabela de
semânticas e estados reais das chamadas pedidas. Foram especificados os
protocolos propostos para dircache e auditoria, mais o threat model de servidor
de diretório. Criado `libcarlio` como membro local do workspace, com
`#![forbid(unsafe_code)]`, API Rust tipada e buffers emprestados via trait
`KernelAbi`.

Isso não implementa o transporte de syscalls em `libcarlio`, não conecta a
crate ao kernel ou ao pegasusDS e não instala canais ou controles de segurança.
O projeto Pegasus DS está em `~/Work/pegasus_ds`, mas o serviço `pegasusd` é
somente um scaffold que imprime uma mensagem e não exercita chamadas de sistema;
logo ainda não há workload de conformidade executável. A ABI Linux geral,
compatibilidade glibc/musl, capabilities
reais, `carlio_sandbox`, hooks LSM, keyring e integração de rede permanecem
pendentes. O gate CPL3 atual também falha no retorno de interrupção dos APs;
logo não se deve expor serviço de rede.

### Evidências

- `cargo fmt --all --check` — passou; [log](logs/pegasusds-abi-fmt-20261118.txt).
- `cargo test --workspace --all-targets` — passou: 159 testes do kernel, 1 de
  `carlio-logs`, 4 de `herm-fs`; a crate não tem testes de comportamento ainda;
  [log](logs/pegasusds-abi-host-20261118.txt).
- `cargo clippy --workspace --all-targets -- -D warnings` — passou;
  [log](logs/pegasusds-abi-clippy-20261118.txt).
- O serviço Pegasus ainda não contém workload (ver
  [`pegasusd`](../../pegasus_ds/services/pegasusd/src/main.rs)); nenhum teste
  QEMU do novo contrato foi executado. Os protocolos/canais também ainda não
  existem no kernel.
- Conectividade NAS/KVM: `ssh -o BatchMode=yes -o ConnectTimeout=8 nas-wifi
  'uname -a'` respondeu Linux x86_64.
- BIOS/UEFI QEMU para a correção de isolamento seguem reprovados nos logs da
  [Fase 2](fase-2-memoria-execucao.md); NAS conectado não substitui a correção
  nem autoriza alegar isolamento completo.

**Estado:** fase aberta. Nenhum critério de aceite funcional do pegasusDS está
fechado. Não houve publicação em registry interno nem alteração de boot.

## Próximos incrementos

1. Corrigir e provar CR3/CPL3/RSP0 e preempção em BSP/APs, BIOS/UEFI e NAS/KVM.
2. Integrar o transporte `KernelAbi` à ABI syscall Linux e criar testes de
   conformidade positivos, negativos e de falha.
3. Implementar capacidades e enforcement `carlio_sandbox`/`carlio_lsm`, com
   auditoria `EPERM` sem matar o processo.
4. Fechar VFS persistente, threads/futex/sinais, ELF dinâmico, TCP/AF_UNIX/IPv6
   e epoll necessários ao workload.
5. Adicionar/publicar o workload pegasusDS, habilitar os canais somente via
   capability + configuração de instalação e executar QEMU BIOS/UEFI e NAS/KVM.

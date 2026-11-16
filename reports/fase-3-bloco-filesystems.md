# Relatório — Fase 3: bloco e filesystems

## Incremento de capacidade 2 TiB (2026-11-15)

## Recovery AHCI e regressão local/NAS (2026-11-16)

O recovery COMRESET dependia de ticks PIT, mas syscalls/probes podiam devolver
o kernel com IF desabilitada. `idt::wait_ticks` agora preserva o IF anterior e
habilita interrupções somente durante a espera limitada, restaurando o estado
ao sair. O teste de disco confirmou escrita/leitura, comando ATA inválido,
COMRESET, reuso do controlador e remontagem/scrub do Hermione em mídia raw
temporária.

Validação: 129 testes host; ISO normal BIOS/UEFI QEMU TCG local e KVM no NAS;
teste `storage-test` também passou local e no NAS/KVM. Logs: [host local](logs/fase-local-host-20261116-110205.txt),
[BIOS local](logs/fase-local-qemu-bios-20261116-110549.log),
[UEFI local](logs/fase-local-qemu-uefi-20261116-110549.log),
[resumo NAS](logs/kernel-subsystems-final-20261116-nas-20261116-110614.txt),
[AHCI NAS/KVM](logs/kernel-subsystems-final-20261116-nas-20261116-110614-filesystem-qemu-kvm.log)
e [AHCI local](logs/hardware-prioridade-ahci-qemu-20261116-final.log).

O teste é de controlador QEMU `8086:2922`, não valida os controladores AHCI
físicos `8086:282a` e `8086:1c02`.

O contrato do formato aceitava `u32::MAX` setores, deixando de fora o volume
exato de 2 TiB (`2^32` setores de 512 bytes). Hermione v3 agora aceita essa
contagem em seu campo de superbloco `u64`, e `herm-fs criar` usa o mesmo limite.
Os extents continuam em `u32`, suficiente para representar o último LBA
(`2^32-1`); o limite de tamanho de arquivo continua separado e menor.

Teste host cria uma imagem esparsa de 2 TiB, grava e relê o último LBA e rejeita
o setor imediatamente após o fim. Outro teste formata e monta um dispositivo
com exatamente `2^32` setores e rejeita capacidade acima disso. Isso verifica o
backend de imagem, não AHCI físico nem gravação em mídia real.

**Estado da fase:** incremento aprovado; fase permanece em andamento. FAT32 e
ext4 não estão montados pelo kernel, e persistem os gates de timeout/remoção,
recuperação em mídia física, criptografia e interoperabilidade documentados
abaixo.

Validação da revisão: 101 testes host passaram localmente e no NAS; boots BIOS/
UEFI em QEMU TCG local e QEMU/KVM NAS passaram com 4 CPUs. O teste da imagem
esparsa está em [host local](logs/fase3-5-incrementos-host-20261115-194208.txt)
e [host NAS](logs/fase3-5-incrementos-nas-20261115-194229.txt); logs seriais:
[BIOS](logs/fase3-5-incrementos-qemu-bios-20261115-194208.log),
[UEFI](logs/fase3-5-incrementos-qemu-uefi-20261115-194208.log),
[BIOS NAS](logs/fase3-5-incrementos-nas-20261115-194229-qemu-kvm-bios.log) e
[UEFI NAS](logs/fase3-5-incrementos-nas-20261115-194229-qemu-kvm-uefi.log).

A revisão posterior, com as operações de unlink no VFS, passou 102 testes host
e repetiu os boots BIOS/UEFI locais e NAS/KVM. Consulte [host local](logs/fase3-5-fechamento-host-20261115-194459.txt),
[host NAS](logs/fase3-5-fechamento-nas-20261115-194519.txt), [BIOS](logs/fase3-5-fechamento-qemu-bios-20261115-194459.log),
[UEFI](logs/fase3-5-fechamento-qemu-uefi-20261115-194459.log), [BIOS NAS](logs/fase3-5-fechamento-nas-20261115-194519-qemu-kvm-bios.log)
e [UEFI NAS](logs/fase3-5-fechamento-nas-20261115-194519-qemu-kvm-uefi.log).

**Estado:** protótipo em evolução. O incremento Hermione v3 passa em mídia
descartável; o leitor FAT32 ainda é uma biblioteca host-only. O kernel não monta
filesystems no boot normal.

## Entregas deste incremento

- `BlockDevice` define setores de 512 bytes, contagem, leitura, escrita e flush.
  O adaptador AHCI QEMU implementa o contrato; o utilitário `herm-fs` também
  opera sobre arquivo-imagem e sincroniza com `sync_all`.
- O boot tenta IDENTIFY em AHCI Intel `8086:282a`/`8086:1c02` e QEMU
  `8086:2922`; escrita continua limitada à build explícita `storage-test`.
  Essa inclusão ainda não foi validada em boot sobre controlador físico.
- Hermione v3 reserva journal redo e diretórios alternados, grava espelhos por
  arquivo e verifica checksums. O mount reaplica journal mais novo; scrub repara
  uma cópia somente quando a cópia-espelho confere. Se as duas cópias divergem
  do checksum esperado, retorna erro.
- `herm-fs` administra imagens: `criar` é exclusivo e não sobrescreve, além de
  `listar`, `gravar`, `ler` e `scrub`. A entrada de arquivo na CLI é limitada a
  64 MiB por operação.
- `Fat32Volume` lê a partição fornecida pelo chamador, lista a raiz e lê nomes
  8.3. Não escreve. Foi exercitado contra imagem sintética esparsa host-only.

## Validação

- 44 testes host passaram: 40 da biblioteca, 1 do gerador de logs e 3 do CLI
  `herm-fs`.
- ISO normal: BIOS/UEFI passou em QEMU TCG local e BIOS/UEFI passou em QEMU/KVM
  no NAS conectado por cabo. O comando de kernel `logs` e a validação das
  tabelas x86_64 também passaram no smoke.
- AHCI + Hermione v3: IDENTIFY/WRITE/READ, erro de controlador, formatar,
  gravar espelho, commit journal, remontar, ler e scrub passaram em imagem
  descartável local e NAS/KVM.
- Evidências locais: [`host`](logs/fase-integracao-concluida-host-20261115-141629.txt),
  [`BIOS`](logs/fase-integracao-concluida-qemu-bios-20261115-141629.log),
  [`UEFI`](logs/fase-integracao-concluida-qemu-uefi-20261115-141629.log) e
  [`AHCI/filesystem`](logs/fase-integracao-concluida-filesystem-qemu-local-20261115.log).
- Evidências NAS/KVM: [`host`](logs/fase-integracao-concluida-nas-20261115-141649.txt),
  [`BIOS`](logs/fase-integracao-concluida-nas-20261115-141649-qemu-kvm-bios.log),
  [`UEFI`](logs/fase-integracao-concluida-nas-20261115-141649-qemu-kvm-uefi.log) e
  [`AHCI/filesystem`](logs/fase-integracao-concluida-nas-20261115-141649-filesystem-qemu-kvm.log).

## Pendências explícitas

- Não há driver Ethernet físico RTL8111/I219-LM, boot físico validado, teste de
  timeout/remoção de disco, nem montagem de volume pelo VFS/kernel.
- FAT32 só lê diretório raiz com nomes curtos 8.3 em host; faltam nomes longos,
  subdiretórios, detecção de partição e integração ao mount normal.
- Driver ext4 ainda não foi implementado.
- Reserva planejada de 16 GiB (meta de 1 GiB logs + até 15 GiB swap) não foi
  particionada. Swap permanece desativado até VM independente estável e bloco
  confiável, conforme a ordem definida.
- Hermione permanece formato experimental incompatível com v2 e não deve
  receber dados reais. A montagem normal, cifra, árvore de diretórios e
  redundância além do espelho local seguem pendentes.

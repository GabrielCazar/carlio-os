# Relatório — Fase 1: inicialização

**Estado:** concluída no escopo x86_64 de inicialização, diagnóstico e prova
experimental AHCI em QEMU, validada localmente e no NAS/KVM. Isso não certifica
hardware físico. AHCI nos controladores do servidor/desenvolvimento, timeout
real e remoção durante I/O permanecem pendentes.

### Correção — diagnóstico AHCI sem disco — 24/11/2026

O teste negativo aguardava um prefixo desatualizado. O harness agora exige
a mensagem emitida pelo kernel e a chegada ao shell. A revalidação da
revisão atual passou em QEMU TCG BIOS e UEFI com a ISO padrão. A ISO de
diagnóstico, criada com `CARLIO_CRASH_TEST=1 CARLIO_STORAGE_TEST=1
./scripts/build-iso.sh`, passou nos quatro cenários de falha e nas duas
execuções de `scripts/test-ahci-qemu.py` (disco descartável e `--without-disk`).

- [Boot BIOS](logs/fechamento-fase-2-20261124-5G-bios-final.log) · [boot UEFI](logs/fechamento-fase-2-20261124-5G-uefi-final.log).
- [Panic](logs/fechamento-fase-1-20261124-crash-panic.log) · [#UD](logs/fechamento-fase-1-20261124-crash-fault.log) · [watchdog](logs/fechamento-fase-1-20261124-crash-watchdog.log) · [double fault](logs/fechamento-fase-1-20261124-crash-doublefault.log).
- [AHCI com disco descartável](logs/fechamento-fase-1-20261124-ahci-disco-final.log) · [AHCI sem disco](logs/fechamento-fase-1-20261124-ahci-sem-disco-final.log).

O fechamento anterior inclui NAS/KVM. A revalidação no NAS ficou pendente
por indisponibilidade dos três endereços SSH configurados. Hardware físico
segue sem validação. [Conectividade NAS](logs/fechamento-fases-1-2-20261124-nas.txt).

### Fechamento final — 16/11/2026

Os gates executáveis da fase e suas dependências foram repetidos após as
correções: `cargo fmt --check`, 159 testes host, Clippy com `-D warnings`, ISO
normal e boots BIOS/UEFI locais, além de 159 testes e BIOS/UEFI no NAS/KVM.
O build diagnóstico `storage-test` passou em mídia raw descartável de 16 MiB
localmente e no NAS/KVM; o teste cobre IDENTIFY, escrita/leitura, erro ATA,
volume Hermione, remontagem e scrub. Logs:

- Host e formato local: [host/fmt](logs/fase1-fechamento-final-host-20261116-190018.txt),
  [Clippy](logs/fase1-fechamento-final-clippy-20261116.txt).
- Boot local: [BIOS](logs/fase1-fechamento-final-qemu-bios-20261116-190018.log),
  [UEFI](logs/fase1-fechamento-final-qemu-uefi-20261116-190018.log).
- NAS/KVM: [host](logs/fase1-final2-nas-20261116-185928.txt),
  [BIOS](logs/fase1-final2-nas-20261116-185928-qemu-kvm-bios.log),
  [UEFI](logs/fase1-final2-nas-20261116-185928-qemu-kvm-uefi.log).
- AHCI/Hermione: [QEMU local](logs/fase1-final-ahci-qemu-local-20261116.log),
  [QEMU/KVM NAS](logs/fase1-final2-nas-20261116-185928-filesystem-qemu-kvm.log).
- Falhas intencionais em build diagnóstico: [panic](logs/fase1-final-crash-watchdog-panic-20261116.log),
  [#UD](logs/fase1-final-crash-watchdog-fault-20261116.log),
  [watchdog](logs/fase1-final-crash-watchdog-watchdog-20261116.log) e
  [double fault](logs/fase1-final-crash-watchdog-doublefault-20261116.log).

Uma primeira execução automatizada do AHCI no NAS foi inválida: usou ISO sem
`storage-test` e não alimentou o probe CPL3 antes de aguardar o prompt. O log
está preservado em [tentativa inválida](logs/fase1-final-nas-20261116-185654-filesystem-qemu-kvm.log).
O harness foi ajustado para alimentar o probe e a repetição com ISO diagnóstica
passou nos logs acima. Toda escrita ocorreu apenas em mídia descartável QEMU
sob `/tmp/carlioos-*`; nenhum dispositivo físico foi acessado.

### Revalidação geral — 16/11/2026

Após a integração do driver de rede QEMU e das últimas mudanças no kernel,
`scripts/test-local-iso.sh` passou com 129 testes host e boots TCG BIOS/UEFI.
`scripts/test-nas.sh` passou com 129 testes host no NAS e boots KVM BIOS/UEFI.
Os boots confirmam novamente Limine, IDT, watchdog, SMP, framebuffer, NIC/DHCP
e runtime CPL3. Eles são regressão do boot normal: o teste dedicado de AHCI em
mídia descartável continua sendo a evidência de I/O e está nos logs de
fechamento abaixo; nenhum controlador físico foi exercitado.

Logs atuais: [host local](logs/fase-local-host-20261116-110549.txt),
[BIOS local](logs/fase-local-qemu-bios-20261116-110549.log),
[UEFI local](logs/fase-local-qemu-uefi-20261116-110549.log),
[host/NAS](logs/kernel-subsystems-final-20261116-nas-20261116-110614.txt),
[BIOS NAS](logs/kernel-subsystems-final-20261116-nas-20261116-110614-qemu-kvm-bios.log) e
[UEFI NAS](logs/kernel-subsystems-final-20261116-nas-20261116-110614-qemu-kvm-uefi.log).
O teste AHCI posterior passou em mídia descartável: [QEMU local](logs/hardware-prioridade-ahci-qemu-20261116-final.log)
e [QEMU/KVM NAS](logs/kernel-subsystems-final-20261116-nas-20261116-110614-filesystem-qemu-kvm.log).

### Fechamento — 15/11/2026

- `scripts/test-local-iso.sh` passou: `cargo fmt --check`, **30 testes host**
  (28 na biblioteca e 2 no utilitário),
  build da ISO e boots BIOS/UEFI no QEMU TCG com terminal serial. Logs:
  [`host`](logs/fase-1-fechamento-host-20261115-132519.txt),
  [`BIOS`](logs/fase-1-fechamento-qemu-bios-20261115-132519.log) e
  [`UEFI`](logs/fase-1-fechamento-qemu-uefi-20261115-132519.log).
- Build `storage-test` e `scripts/test-ahci-qemu.py` passaram em disco raw
  temporário: IDENTIFY, WRITE/READ com comparação, erro ATA inválido e
  formatação/montagem/leitura do volume Hermione. Log:
  [`AHCI QEMU descartável`](logs/fase-1-fechamento-ahci-qemu-20261115.log).
- No NAS cabeado, `scripts/test-nas.sh` passou: 30 testes host, estado da
  ferramenta Hermione e boots BIOS/UEFI em QEMU/KVM. Logs:
  [`resumo NAS`](logs/fase-1-fechamento-nas-20261115-132801.txt),
  [`BIOS`](logs/fase-1-fechamento-nas-20261115-132801-qemu-kvm-bios.log) e
  [`UEFI`](logs/fase-1-fechamento-nas-20261115-132801-qemu-kvm-uefi.log).
- Os limites de polling são finitos, mas não se induziu timeout real nem
  remoção a quente durante I/O; controladores físicos também não foram
  testados. Estes limites não são alegados como suporte.

## Checklist da fase

### Boot e plataforma

- [x] Inicialização Limine em BIOS legado e UEFI, validada no QEMU local e NAS/KVM ([local BIOS/UEFI](logs/fase1-fechamento-final-qemu-bios-20261116-190018.log), [NAS BIOS/UEFI](logs/fase1-final2-nas-20261116-185928-qemu-kvm-bios.log)).
- [x] Coletar e validar mapa de memória, HHDM e bases virtual/física do kernel ([local](logs/fase1-fechamento-final-qemu-bios-20261116-190018.log)).
- [x] Exibir splash e terminal serial em pt_BR ([local](logs/fase1-fechamento-final-qemu-bios-20261116-190018.log)).
- [x] Panic imprime causa e localização de arquivo/linha/coluna; panic intencional
  passou no QEMU com build de teste ([log](logs/fase1-final-crash-watchdog-panic-20261116.log)).
- [x] Instalar IDT para exceções 0–31 e capturar #UD no QEMU com código de erro,
  RIP, CS, RFLAGS e registradores gerais; page fault também reporta CR2 ([#UD](logs/fase1-final-crash-watchdog-fault-20261116.log)).
- [x] Detectar ausência de progresso com watchdog PIT; teste de loop travado passou no QEMU ([log](logs/fase1-final-crash-watchdog-watchdog-20261116.log)).
- [x] Usar IST dedicada de 16 KiB para double fault; teste intencional passou no QEMU ([log](logs/fase1-final-crash-watchdog-doublefault-20261116.log)).
- [x] Exportar dumps serial para arquivos de log do host nos testes QEMU ([BIOS](logs/fase1-fechamento-final-qemu-bios-20261116-190018.log)).
- [x] Persistir os dumps de teste em `reports/logs/` pelo harness serial do QEMU ([crash-tests](logs/fase1-final-crash-watchdog-doublefault-20261116.log)).

### PCI e dispositivos

- [x] Enumerar PCI sem alterar dispositivos e classificar vídeo, USB, rede e AHCI ([log](logs/fase1-fechamento-final-qemu-bios-20261116-190018.log)).
- [x] Reconhecer os IDs AHCI da máquina (`8086:282a`), NAS (`8086:1c02`) e QEMU (`8086:2922`) ([inventário](logs/kernel-subsystems-final-20261116-nas-20261116-110614-qemu-kvm-bios.log)).
- [x] Inicializar AHCI QEMU e executar IDENTIFY/READ/WRITE por DMA com espera limitada ([local](logs/fase1-final-ahci-qemu-local-20261116.log), [NAS/KVM](logs/fase1-final2-nas-20261116-185928-filesystem-qemu-kvm.log)).
- [x] Testar escrita/leitura, ausência de disco e erro do controlador com imagem raw descartável no QEMU ([positivo](logs/fase1-final-ahci-qemu-local-20261116.log), [sem disco](logs/fase1-fechamento-final-qemu-bios-20261116-190018.log), [NAS/KVM](logs/fase1-final2-nas-20261116-185928-filesystem-qemu-kvm.log)).
- Adiado para validação de hardware: I/O nos controladores Intel físicos (`8086:282a` e `8086:1c02`). Não faz parte do gate de boot/QEMU concluído.
- Planejado após driver Ethernet: pilha TCP/IP e sockets para protocolos remotos.
- Planejados por necessidade: vídeo/framebuffer, USB, Ethernet, Wi-Fi, Bluetooth e áudio; detecção PCI não declara suporte.

### Validação e limites

- [x] Smoke tests BIOS/UEFI no QEMU local/TCG e NAS/KVM; testes host anexados em logs ([local](logs/fase1-fechamento-final-host-20261116-190018.txt), [NAS](logs/fase1-final2-nas-20261116-185928.txt)).
- [x] Validar panic, #UD, watchdog e double fault intencionais no QEMU ([logs](logs/fase1-final-crash-watchdog-panic-20261116.log)).
- [x] Validar AHCI positivo/negativo e erro de comando no QEMU; timeout é limitado, mas não foi induzido neste teste ([positivo](logs/fase1-final-ahci-qemu-local-20261116.log), [sem disco](logs/fase1-fechamento-final-qemu-bios-20261116-190018.log)).
- Posterior à fase de desktop: operação com interface gráfica ativa no NAS.
- [x] Fechar o escopo de boot, diagnóstico e dependência mínima de bloco QEMU após testes e logs ([relatório final](#fechamento-final--16112026)).

Os comandos de falha são compilados apenas com `crash-test`. A IDT trata
exceções 0–31; o watchdog PIT usa IRQ0 e registra o frame interrompido após 5 s
sem progresso; double fault usa IST própria. Os dumps são enviados pela serial
e persistidos pelo harness de teste no host. O kernel não grava dump em seu
próprio volume; faults encerram a CPU, e o panic Rust não dispõe dos registradores do
ponto exato da chamada.

- Testes host: 14 passaram localmente e no NAS. ISO normal e smoke BIOS/UEFI
  passaram no QEMU local e no NAS/KVM.
- Capturas de panic, #UD, watchdog e double fault passaram em build de
  diagnóstico; dumps: [`logs/fase-1-crash-qemu-panic.log`](logs/fase-1-crash-qemu-panic.log),
  [`logs/fase-1-crash-qemu-ud.log`](logs/fase-1-crash-qemu-ud.log),
  [`logs/fase-1-crash-qemu-watchdog.log`](logs/fase-1-crash-qemu-watchdog.log) e
  [`logs/fase-1-crash-qemu-doublefault.log`](logs/fase-1-crash-qemu-doublefault.log).
- AHCI em QEMU: disco descartável de 16 MiB, IDENTIFY reportou 32.768 setores,
  escrita/leitura do LBA 1 comparou os 512 bytes e um comando ATA inválido
  retornou erro do controlador. Sem disco, recebeu `dispositivo nao encontrado`.
  A recuperação/reset por COMRESET foi implementada depois na fase 2 e passou
  em QEMU local e NAS/KVM. Controladores físicos e timeout continuam pendentes.
  Logs:
  [`logs/fase-1-ahci-qemu-disposable.log`](logs/fase-1-ahci-qemu-disposable.log) e
  [`logs/fase-1-ahci-qemu-sem-disco.log`](logs/fase-1-ahci-qemu-sem-disco.log).
- O driver atualmente aceita somente o controlador QEMU `8086:2922`. Logs de
  boot NAS/KVM sem disco de escrita: [`logs/fase-1-ahci-nas.txt`](logs/fase-1-ahci-nas.txt),
  [`logs/fase-1-ahci-nas-bios.log`](logs/fase-1-ahci-nas-bios.log) e
  [`logs/fase-1-ahci-nas-uefi.log`](logs/fase-1-ahci-nas-uefi.log). Smoke local:
  [`logs/fase-1-ahci-local.log`](logs/fase-1-ahci-local.log).
- O marco desta fase está fechado. A expansão para `8086:282a`, `8086:1c02`,
  VGA, USB, Ethernet, Wi-Fi, Bluetooth e áudio segue como trabalho posterior;
  nenhum suporte físico é afirmado por estes testes QEMU.

## Entregue

- Kernel mínimo `no_std` em Rust para x86_64 e entrada Limine.
- Terminal COM1 em pt_BR com os comandos `ajuda`, `sobre`, `usuario`,
  `permissoes`, `erros`, `limpar` e `parar`; o comando `sobre` mostra o nome e
  a autoria do sistema.
- Splash serial simples em ASCII com a assinatura “carlIO OS | Criado por João
  G.R.Santos”.
- Modelo de permissões demonstrativo, identificado como não isolante.
- ELF com segmentos de carga em páginas separadas; ISO híbrida BIOS/UEFI.
- Teste automatizado de serial no QEMU e teste obrigatório no NAS/KVM.

## Histórico de validações anteriores

Os números e logs abaixo registram execuções anteriores ao fechamento de
15/11/2026. O estado atual e os resultados mais recentes estão na seção
“Fechamento”.

- Testes host: 3 passaram localmente e 3 passaram no NAS.
- QEMU local TCG BIOS: boot e comandos `ajuda`, `sobre`, `parar` passaram.
- NAS x86_64 QEMU/KVM BIOS: boot e os mesmos comandos passaram.
- Log do NAS: [`logs/fase-1-nas-20261114-040745.txt`](logs/fase-1-nas-20261114-040745.txt)
  e [`logs/fase-1-nas-20261114-040745-qemu.log`](logs/fase-1-nas-20261114-040745-qemu.log).
- Log local: `build/qemu-local.log`.
- Build bare-metal com Rust nightly 1.102.0 e ISO com xorriso passaram.
- UEFI: boot passou localmente e no NAS/KVM.
- NAS: o QEMU GTK foi iniciado na sessão Plasma, mas o monitor reportou a VM como pausada (shutdown); a visualização de uma VM ativa permanece pendente.
- Revalidação BIOS/UEFI e testes host: [`logs/fase-2-nas-20261114-043214.txt`](logs/fase-2-nas-20261114-043214.txt), [`logs/fase-2-nas-20261114-043214-qemu-kvm-bios.log`](logs/fase-2-nas-20261114-043214-qemu-kvm-bios.log) e [`logs/fase-2-nas-20261114-043214-qemu-kvm-uefi.log`](logs/fase-2-nas-20261114-043214-qemu-kvm-uefi.log).

## Limites

Não há ainda criptografia de disco, Hermione persistente, SQLite, ring 3 ou
isolamento. O driver AHCI atual é experimental e limitado ao QEMU; o terminal e
todo o kernel executam no nível 0.

## Escopo adicionado ao reabrir a fase

- Solicitar e validar mapa de memória do Limine/HHDM e definir acesso a memória
  física e regiões de dispositivo.
- Enumerar PCI e detectar os controladores AHCI Intel da máquina (`8086:282a`)
  e do NAS (`8086:1c02`), incluindo dispositivo ausente e IDs não suportados.
- Implementar I/O AHCI por polling e leitura/escrita de setores; validar dados
  com disco descartável QEMU e propagar erros sem espera infinita.
- Registrar logs seriais e repetir os testes locais e no NAS/KVM, em BIOS e
  UEFI, antes de encerrar novamente esta fase.
- O kernel permanece experimental: o driver inicial não implica criptografia,
  isolamento, sistema de arquivos em disco ou segurança para dados reais.
- A matriz dos dispositivos e a ordem dos próximos drivers estão em
  [`../docs/HARDWARE-BASE.md`](../docs/HARDWARE-BASE.md).

## Implementação inicial desta retomada

- Varredura PCI por leitura de configuração, identificando classes AHCI/SATA,
  vídeo, USB e rede sem habilitar nem modificar dispositivos.
- Serial usa espera limitada; erros de timeout e entrada excessiva vão para um
  buffer fixo consultável pelo comando `erros`.
- Panic registra o evento e abre um modo serial restrito para diagnóstico.
- O kernel agora solicita mapa de memória e HHDM do Limine, enumera classes PCI
  de armazenamento/vídeo/USB/rede e mantém um log de erros com capacidade fixa.
- Serial usa timeout; o comando `erros` lê o log e o panic muda para diagnóstico
  restrito. Isso não isola drivers, que continuam em ring 0.
- A tela inicial e o comando `sobre` apresentam a assinatura solicitada; o
  smoke test confere autoria, descrição e sequência do terminal.
- Ainda não há framebuffer/VGA, AHCI funcional nos controladores físicos, USB,
  Ethernet, Wi-Fi ou Bluetooth. A detecção PCI não equivale a suporte nesses
  dispositivos.
- Testes host: 11 passaram localmente e no NAS. QEMU BIOS/UEFI passou localmente
  e no NAS/KVM; inventário do QEMU detectou SATA/AHCI, vídeo e rede. A interface
  GTK ativa no Plasma segue pendente. Logs: [`logs/fase-1-nas-20261114-045426.txt`](logs/fase-1-nas-20261114-045426.txt),
  [`logs/fase-1-nas-20261114-045426-qemu-kvm-bios.log`](logs/fase-1-nas-20261114-045426-qemu-kvm-bios.log),
  [`logs/fase-1-nas-20261114-045426-qemu-kvm-uefi.log`](logs/fase-1-nas-20261114-045426-qemu-kvm-uefi.log) e `build/qemu-local-*-phase1.log`.

## Continuação — classificação dos controladores AHCI

- A descoberta PCI agora distingue os modelos Intel RST da máquina (`8086:282a`),
  Intel do NAS (`8086:1c02`) e AHCI padrão do QEMU (`8086:2922`). Exige também
  classe/subclasse SATA/AHCI e interface de programação AHCI (`01`). Outros IDs
  AHCI são marcados explicitamente como não suportados; interfaces incompatíveis
  não são tratadas como AHCI.
- Na etapa de classificação, a saída serial apenas reconhecia os IDs; o driver
  QEMU experimental foi implementado na continuação registrada abaixo.
- Validação: 12 testes host passaram localmente; build da ISO passou; smoke test
  QEMU local TCG BIOS passou. No NAS, 12 testes host e boots QEMU/KVM BIOS e UEFI
  passaram. O inventário exibiu `8086:2922` como modelo QEMU reconhecido.
- Logs: [`logs/fase-1-nas-20261114-050507.txt`](logs/fase-1-nas-20261114-050507.txt),
  [`logs/fase-1-nas-20261114-050507-qemu-kvm-bios.log`](logs/fase-1-nas-20261114-050507-qemu-kvm-bios.log),
  [`logs/fase-1-nas-20261114-050507-qemu-kvm-uefi.log`](logs/fase-1-nas-20261114-050507-qemu-kvm-uefi.log) e
  [`../build/qemu-local-phase1-ahci.log`](../build/qemu-local-phase1-ahci.log).
- Limite desta etapa: reconhecimento de IDs não implementa o driver AHCI nem
  prova compatibilidade de leitura/escrita. Próximo marco: reservar memória DMA
  alinhada via HHDM, implementar IDENTIFY/READ/WRITE com timeout e exercitar
  exclusivamente um disco descartável QEMU.

## Informações de firmware e endereços para inicialização de dispositivos

- O kernel solicita ao Limine o mapa de memória, o HHDM e as bases física e
  virtual de carregamento do próprio kernel. A conversão de endereço virtual
  para físico usa aritmética verificada contra underflow/overflow; endereços
  HHDM também têm soma verificada. Esses dados são a base para localizar
  estruturas do kernel aptas a DMA.
- BIOS legado e UEFI são caminhos de firmware usados para carregar o kernel;
  as informações acima são entregues pelo Limine após o boot. Elas não são
  chamadas de BIOS para iniciar dispositivos, nem habilitam o controlador PCI,
  não mapeiam MMIO e ainda não iniciam o AHCI.
- O inventário PCI fornece BDF, vendor/device ID, classe e interface, usados
  para selecionar um driver. Inicialização do dispositivo (BAR, bus mastering,
  filas/comandos AHCI e DMA) segue pendente e deve ser testada apenas com disco
  descartável no QEMU.
- Validação desta alteração preparatória: 14 testes host passaram localmente;
  no NAS passaram 14 testes (12 na biblioteca e 2 no CLI). ISO reconstruída e
  smoke tests BIOS/UEFI passaram localmente em TCG e no NAS em KVM. Os logs
  comprovam que o firmware carregou a imagem e que Limine entregou HHDM e bases
  físico/virtual do kernel. Ainda não houve inicialização AHCI nem teste de
  disco descartável.
- Logs locais: [`../build/qemu-local-ahci-prep-bios.log`](../build/qemu-local-ahci-prep-bios.log)
  e [`../build/qemu-local-ahci-prep-uefi.log`](../build/qemu-local-ahci-prep-uefi.log).
- Logs NAS: [`logs/fase-1-nas-20261114-051001.txt`](logs/fase-1-nas-20261114-051001.txt),
  [`logs/fase-1-nas-20261114-051001-qemu-kvm-bios.log`](logs/fase-1-nas-20261114-051001-qemu-kvm-bios.log)
  e [`logs/fase-1-nas-20261114-051001-qemu-kvm-uefi.log`](logs/fase-1-nas-20261114-051001-qemu-kvm-uefi.log).

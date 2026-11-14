# Relatório — Fase 1: inicialização

**Estado:** concluída no escopo de inicialização x86_64, diagnóstico do kernel e
dependência mínima de armazenamento para a fase 2. O driver AHCI permanece
experimental e validado somente no QEMU com imagem descartável; suporte aos
controladores SATA físicos e aos demais dispositivos fica fora deste marco.

## Checklist da fase

### Boot e plataforma

- [x] Inicialização Limine em BIOS legado e UEFI, validada no QEMU local e NAS/KVM.
- [x] Coletar e validar mapa de memória, HHDM e bases virtual/física do kernel.
- [x] Exibir splash e terminal serial em pt_BR.
- [x] Panic imprime causa e localização de arquivo/linha/coluna; panic intencional
  passou no QEMU com build de teste.
- [x] Instalar IDT para exceções 0–31 e capturar #UD no QEMU com código de erro,
  RIP, CS, RFLAGS e registradores gerais; page fault também reporta CR2.
- [x] Detectar ausência de progresso com watchdog PIT; teste de loop travado passou no QEMU.
- [x] Usar IST dedicada de 16 KiB para double fault; teste intencional passou no QEMU.
- [x] Exportar dumps serial para arquivos de log do host nos testes QEMU.
- [x] Persistir os dumps de teste em `reports/logs/` pelo harness serial do QEMU.

### PCI e dispositivos

- [x] Enumerar PCI sem alterar dispositivos e classificar vídeo, USB, rede e AHCI.
- [x] Reconhecer os IDs AHCI da máquina (`8086:282a`), NAS (`8086:1c02`) e QEMU (`8086:2922`).
- [x] Inicializar AHCI QEMU e executar IDENTIFY/READ/WRITE por DMA com espera limitada.
- [x] Testar escrita/leitura, ausência de disco e erro do controlador com imagem raw descartável no QEMU.
- [ ] Validar I/O nos controladores físicos Intel local (`8086:282a`) e NAS (`8086:1c02`).
- [ ] Implementar pilha TCP/IP e sockets como dependência dos protocolos remotos planejados para a fase 6.
- [ ] Implementar vídeo/framebuffer e os drivers USB, Ethernet, Wi-Fi, Bluetooth e áudio como trabalho posterior.

### Validação e limites

- [x] Smoke tests BIOS/UEFI no QEMU local/TCG e NAS/KVM; testes host anexados em logs.
- [x] Validar panic, #UD, watchdog e double fault intencionais no QEMU.
- [x] Validar AHCI positivo/negativo e erro de comando no QEMU; timeout é limitado, mas não foi induzido neste teste.
- [ ] Confirmar operação com interface gráfica ativa no NAS.
- [x] Fechar o escopo de boot, diagnóstico e dependência mínima de bloco QEMU após testes e logs.

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

## Resultados

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

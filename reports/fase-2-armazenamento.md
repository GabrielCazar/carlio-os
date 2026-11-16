# Relatório — Fase 2: armazenamento protegido e Hermione

**Estado:** Hermione v2 é um protótipo de armazenamento em desenvolvimento ativo. Duas cópias de diretório, flush ordenado e recuperação simulada de gravação interrompida passaram em host; o fluxo de formato/montagem/escrita/leitura e AHCI passou em QEMU local com mídia descartável. Isso ainda não é journal geral nem teste real de perda de energia. O kernel normal não monta o volume; não usar com dados reais.

## Atualização — validação de diretório

- `mount` agora rejeita nomes vazios ou fora do conjunto ASCII permitido, bytes
  não nulos após o terminador, nomes duplicados e extents maiores que o número
  de setores exigido pelo tamanho registrado. Isso limita metadados malformados
  mesmo quando o CRC estrutural do diretório confere.
- Testes host incluem diretório duplicado, nome inválido e extent superdimensionado.
- `cargo test --all-targets`: validação mais recente teve 30 testes aprovados
  (28 biblioteca e 2 CLI). Saída:
  [`logs/fase-2-recovery-host-20261115-131800.txt`](logs/fase-2-recovery-host-20261115-131800.txt).
- O ciclo local host/build/boot BIOS+UEFI e o teste Hermione/AHCI em QEMU TCG
  passaram. Logs: [`logs/fase-2-recovery-qemu-bios-20261115-131800.log`](logs/fase-2-recovery-qemu-bios-20261115-131800.log),
  [`logs/fase-2-recovery-qemu-uefi-20261115-131800.log`](logs/fase-2-recovery-qemu-uefi-20261115-131800.log) e
  [`logs/fase-2-recovery-storage-qemu-local.log`](logs/fase-2-recovery-storage-qemu-local.log).
- `scripts/test-nas.sh` foi tentado; SSH retornou `No route to host` para
  `192.168.3.143`. Conforme orientação do usuário, a verificação NAS fica
  pendente enquanto a conexão estiver indisponível.

## Escopo

Camada de bloco em Rust (AHCI e, depois, VirtIO block), fonte de entropia, volume versionado com
criptografia autenticada, derivação de chave por senha e desbloqueio durante o
boot. Hermione será um filesystem Rust inspirado em ZFS: copy-on-write,
checksums, snapshots, scrub periódico e reparo automático apenas quando houver
uma cópia redundante validada. SQLite integrado fica acima do filesystem e não
armazena segredos. O formato não será interoperável com pools ZFS existentes.

## Critérios de aceite

- Senha correta abre volume de teste; senha incorreta não revela nem monta dados.
- Corrupção, truncamento e cabeçalho incompatível falham com segurança.
- Persistência verificada após reiniciar QEMU.
- Chaves não aparecem em logs, banco ou imagem e são limpas ao bloquear.
- Scrub periódico retoma após reinício; reparo usa cópia validada e caso sem
  redundância é reportado claramente.
- Benchmarks comparam unidade única e espelho sem sacrificar integridade.
- Testes e logs anexados a este relatório.

## Primeira fatia implementada

- Nome definido: Hermione. Utilitário CLI definido como `herm-fs`, com
  subcomandos iniciais `help` e `status`; ele reporta explicitamente que ainda
  não existe volume persistente.

- Armazenamento fixo em memória com blocos lógicos e gravações copy-on-write:
  uma escrita nova só publica a raiz depois de copiar o bloco e calcular seu CRC.
- Snapshots imutáveis da raiz e restauração da raiz selecionada.
- Leitura verifica CRC32; `scrub` percorre os blocos alcançados pela raiz ativa
  e acusa corrupção. O CRC é apenas detector de erro acidental, não autenticação.
- Testes host cobrem escrita/restauração, corrupção, scrub e índices inválidos.

## Início da integração de dispositivo

- A primeira mídia de teste usa AHCI QEMU com DMA; não grava no NAS nem em
  unidades físicas. A feature `storage-test` habilita um comando de teste que
  grava um padrão no LBA 1 da imagem raw descartável e confere os 512 bytes após
  leitura.
- IDENTIFY reportou 32.768 setores para a imagem de 16 MiB. O cenário sem disco
  foi rejeitado com `dispositivo nao encontrado` sem espera infinita.
- Esta integração ainda é um teste do driver, não um `BlockDevice` genérico nem
  um volume Hermione montado. Escritas não estão disponíveis na imagem normal.
- Logs: [`logs/fase-1-ahci-qemu-disposable.log`](logs/fase-1-ahci-qemu-disposable.log),
  [`logs/fase-1-ahci-qemu-sem-disco.log`](logs/fase-1-ahci-qemu-sem-disco.log) e
  [`logs/fase-1-ahci-nas.txt`](logs/fase-1-ahci-nas.txt) (boot NAS sem disco de escrita).

## Limites atuais

Não há interface de bloco genérica, volume persistente, journal/commit resistente a perda de
energia, redundância ou autorreparo, scrub retomável, cifra, KDF, fonte de
entropia, desbloqueio no boot ou SQLite. A tabela e os blocos são voláteis e de
capacidade fixa; este protótipo não deve guardar dados reais.

## Validação desta etapa

- `cargo test --all-targets`: 9 testes passaram localmente (7 na biblioteca e 2 no CLI).
- `herm-fs --help` e `herm-fs status`: passaram localmente; `status` informa que não há volume persistente.
- `scripts/build-iso.sh`: build e geração da ISO passaram.
- `scripts/test-nas.sh`: 9 testes host, execução de `herm-fs status` e boots
  BIOS/UEFI no QEMU/KVM do NAS passaram. Os boots validam a imagem atual, não
  exercitam o armazenamento protótipo, ainda não conectado ao kernel.
- Saídas: [`logs/fase-2-nas-20261114-043214.txt`](logs/fase-2-nas-20261114-043214.txt),
  [`logs/fase-2-nas-20261114-043214-qemu-kvm-bios.log`](logs/fase-2-nas-20261114-043214-qemu-kvm-bios.log) e
  [`logs/fase-2-nas-20261114-043214-qemu-kvm-uefi.log`](logs/fase-2-nas-20261114-043214-qemu-kvm-uefi.log).

## Atualização — início da camada de bloco

- O driver AHCI experimental da fase 1 foi validado como base para a próxima
  etapa: `IDENTIFY`, gravação e leitura DMA em imagem raw descartável QEMU de
  16 MiB; 32.768 setores identificados e comparação dos 512 bytes do LBA 1.
- QEMU sem disco foi rejeitado com erro de dispositivo ausente. O kernel normal
  não oferece o comando de escrita usado pelo build `storage-test`.
- `cargo test --all-targets`: 14 testes passaram localmente e no NAS. Boots da
  imagem normal BIOS/UEFI no QEMU local e NAS/KVM passaram sem mídia gravável
  anexada ao NAS.
- A camada ainda não oferece interface `BlockDevice`, volume Hermione montado,
  persistência após reboot ou criptografia. O início da fase 2 é prototipagem e
  validação de fronteira de bloco; critérios de volume seguro seguem pendentes.
- Logs AHCI: [`logs/fase-1-ahci-qemu-disposable.log`](logs/fase-1-ahci-qemu-disposable.log),
  [`logs/fase-1-ahci-qemu-sem-disco.log`](logs/fase-1-ahci-qemu-sem-disco.log),
  [`logs/fase-1-ahci-nas.txt`](logs/fase-1-ahci-nas.txt),
  [`logs/fase-1-ahci-nas-bios.log`](logs/fase-1-ahci-nas-bios.log) e
  [`logs/fase-1-ahci-nas-uefi.log`](logs/fase-1-ahci-nas-uefi.log).

## Atualização — formato Hermione v1 e teste AHCI

- `src/hermione.rs` agora define `BlockDevice` (setores de 512 bytes) e o
  formato de volume experimental: superbloco em LBA 0, diretório fixo com 12
  entradas em LBA 1 e extents contíguos a partir do LBA 2.
- Implementados `format`, `mount`, `list`, `write_file`, `read_file` e
  `delete_file`. Há CRC32 para superbloco, diretório e conteúdo dos arquivos;
  montagem valida versão, limites e sobreposição de extents. Nomes aceitam
  somente ASCII alfanumérico, ponto, sublinhado e hífen.
- AHCI QEMU implementa a interface de bloco na feature `storage-test`. O comando
  `fs-teste` formata a unidade, grava `teste.txt`, desmonta/remonta e valida
  bytes e checksum. O cenário automatizado faz o teste Hermione antes do teste
  AHCI que injeta um comando inválido, pois o controlador ainda não implementa
  recuperação completa do canal após erro.
- A imagem raw descartável local (16 MiB) passou o fluxo Hermione e o teste
  AHCI `IDENTIFY`/`WRITE`/`READ`/erro induzido. Log serial:
  [`logs/fase-2-filesystem-qemu-disposable.log`](logs/fase-2-filesystem-qemu-disposable.log).
- `cargo test --all-targets`: 18 testes host passaram (16 biblioteca e 2 CLI).
  Saída: [`logs/fase-2-filesystem-host.txt`](logs/fase-2-filesystem-host.txt).
- `scripts/test-nas.sh`: 18 testes host remotos e boots BIOS/UEFI QEMU/KVM
  passaram; os boots deste script não têm disco gravável anexado. Logs NAS
  correspondentes são listados abaixo.
- Criada no NAS a imagem descartável
  `/tmp/carlioos-plasma/carlioos-filesystem-test.qcow2`, QCOW2 de 20 GiB
  (esparsa), ligada como SATA/AHCI à VM gráfica CarlioOS com `-boot order=d`.
  A BIOS/firmware inicia pela ISO; Limine carrega o kernel, que enumera o
  controlador PCI AHCI integrado ao Q35. O processo QEMU ficou ativo no Plasma
  para teste manual; pressionar `Ctrl+Alt+3` abre o console serial, e o comando
  `fs-teste` está disponível somente nesta ISO de teste.

## Limites da fatia v1

- Não há journal, transação atômica diante de falta de energia, recuperação de
  commit, alocador persistente, hierarquia de diretórios, concorrência,
  permissões, cache ou montagem no boot normal.
- Escrita de arquivo contígua e tabela fixa limitam o volume a 12 arquivos e
  tamanho de arquivo representável por 32 bits. CRC32 detecta corrupção
  acidental; não autentica dados.
- O formato é incompatível com FAT, ext4 e ZFS; sem cifra, KDF, chave ou
  redundância/autorreparo. Não guardar dados reais e não anexar discos físicos.
- O teste do diretório duplo simula gravação parcial; cortes de energia reais,
  cache volátil de firmware e garantia de atomicidade de setor seguem pendentes.

## Atualização — formato Hermione v2, flush e recuperação AHCI

- O formato foi elevado para v2: superbloco registra capacidade do volume;
  diretórios alternam entre LBAs 1 e 2, carregam geração de 64 bits e CRC do
  setor. Dados começam no LBA 3. Volumes v1 são incompatíveis e devem ser
  reformatados; todos os testes usam mídia descartável.
- Cada gravação escreve um extent novo, faz `FLUSH CACHE`, publica a cópia
  inativa do diretório e faz novo flush. Substituições preservam o extent antigo
  até o commit. A montagem escolhe a geração válida mais recente e recua para a
  anterior se a nova cópia estiver parcial/corrompida.
- `read_file` valida todos os bytes antes de alterar o buffer de saída; `scrub`
  verifica o checksum de cada arquivo; first-fit agora percorre os extents em
  vez de testar cada setor candidato.
- O driver AHCI implementa `flush` com ATA FLUSH CACHE EXT e recuperação de erro
  via parada do engine, COMRESET, espera por PIT limitada, limpeza de SERR/IS e
  reconfiguração do port. O cenário testa Hermione, injeta erro de task file e
  repete Hermione após o reset do canal.
- `cargo test --all-targets`: 21 testes passaram (19 biblioteca, 2 CLI),
  incluindo dados antigos preservados após substituição interrompida, diretório
  rasgado, corrupção, scrub e ordem de flush.
- QEMU local TCG e NAS/KVM passaram formatação v2, flush, montagem, arquivo e
  checksum; a sequência posterior ao erro AHCI também passou. Boot sem disco
  continua sendo testado como caso negativo.
- A VM gráfica do NAS foi reiniciada com a ISO v2 e usa o mesmo QCOW2 de 20
  GiB. O disco foi preservado durante a troca de ISO; depois, o usuário
  confirmou que o teste local v2 passou. A confirmação manual anterior foi na
  ISO v1. O harness NAS/KVM validou a mesma sequência.
- Logs finais: [`logs/fase-2-filesystem-host.txt`](logs/fase-2-filesystem-host.txt),
  [`logs/fase-2-filesystem-qemu-disposable.log`](logs/fase-2-filesystem-qemu-disposable.log),
  [`logs/fase-2-nas-20261114-063733-filesystem-qemu-kvm.log`](logs/fase-2-nas-20261114-063733-filesystem-qemu-kvm.log),
  [`logs/fase-2-nas-20261114-063733-qemu-kvm-bios.log`](logs/fase-2-nas-20261114-063733-qemu-kvm-bios.log) e
  [`logs/fase-2-nas-20261114-063733-qemu-kvm-uefi.log`](logs/fase-2-nas-20261114-063733-qemu-kvm-uefi.log).

## Checklist — fase 2

- [x] Definir contrato de bloco em setores de 512 bytes.
- [x] Criar superbloco, versão e CRC estrutural.
- [x] Criar diretório fixo e validar nomes, limites e extents sobrepostos.
- [x] Implementar formatar, montar, listar, gravar, ler e excluir arquivo.
- [x] Validar persistência após desmontagem/remontagem em testes host.
- [x] Detectar corrupção de conteúdo e metadados nos testes host.
- [x] Alternar duas gerações de diretório e recuperar a anterior após escrita parcial.
- [x] Ordenar dados/metadata com `flush` e testar a sequência em QEMU.
- [x] Scrub do volume e proteção do buffer de saída contra conteúdo corrompido.
- [x] Recuperar AHCI por COMRESET e executar novo teste de filesystem após o erro.
- [x] Integrar bloco AHCI à build `storage-test` e testar em imagem descartável
  QEMU local.
- [x] Criar e anexar QCOW2 esparsa de 20 GiB à VM de teste NAS via SATA/AHCI.
- [x] Executar `fs-teste` manualmente na VM gráfica v1 com disco de 20 GiB; o
  usuário confirmou aprovação.
- [x] Executar o teste local Hermione v2; usuário confirmou que passou.
- [x] Capturar e anexar a saída serial do teste v2 local em QEMU:
  [`logs/fase-2-recovery-storage-qemu-local.log`](logs/fase-2-recovery-storage-qemu-local.log).
- [ ] Cobrir timeout e remoção do dispositivo durante I/O em QEMU.
- [ ] Implementar journal geral e teste de corte de energia durante cada etapa;
  a injeção host de falhas de escrita/flush não substitui corte de energia QEMU.
- [ ] Integrar montagem somente após estabelecer descoberta/seleção segura do
  dispositivo de boot e separação entre mídia do sistema e mídia de dados.
- [ ] Implementar árvore de diretórios, alocação fragmentada, permissões e
  operações concorrentes.
- [ ] Projetar entropia, KDF e criptografia autenticada antes de alegar volume
  protegido; depois validar senha correta/incorreta e limpeza de chaves.

## Logs desta atualização

- Host: [`logs/fase-2-filesystem-host.txt`](logs/fase-2-filesystem-host.txt)
- QEMU local, imagem raw descartável:
  [`logs/fase-2-filesystem-qemu-disposable.log`](logs/fase-2-filesystem-qemu-disposable.log)
- NAS/KVM filesystem: [`logs/fase-2-nas-20261114-064105-filesystem-qemu-kvm.log`](logs/fase-2-nas-20261114-064105-filesystem-qemu-kvm.log)
- NAS/KVM BIOS: [`logs/fase-2-nas-20261114-064105-qemu-kvm-bios.log`](logs/fase-2-nas-20261114-064105-qemu-kvm-bios.log)
- NAS/KVM UEFI: [`logs/fase-2-nas-20261114-064105-qemu-kvm-uefi.log`](logs/fase-2-nas-20261114-064105-qemu-kvm-uefi.log)
- Resumo NAS v2: [`logs/fase-2-nas-20261114-064105.txt`](logs/fase-2-nas-20261114-064105.txt)
- Confirmação do teste manual NAS:
  [`logs/fase-2-filesystem-nas-manual.txt`](logs/fase-2-filesystem-nas-manual.txt)

## Atualização — adaptador de bloco e integração de mounts (2026-11-16)

`AhciBlockDevice` agora implementa o contrato `BlockDevice`, serializa comandos
de leitura/escrita/flush e rejeita LBAs fora da capacidade IDENTIFY. O teste
Hermione em `storage-test` passou a usar esse adaptador compartilhado. Em mídia
raw descartável, host (135 testes), QEMU TCG local e QEMU/KVM NAS passaram; os
testes dedicados confirmaram IDENTIFY, leitura/escrita, erro ATA, remontagem e
scrub. Isso valida apenas QEMU/KVM: não houve escrita nem teste em disco físico.

Ainda não foi implementado parser GPT/MBR, seleção de partição, montagem no boot,
scrub com cursor persistente nem cifra/KDF/gestão persistente de chaves. Esses
gates continuam pendentes antes de qualquer teste de escrita em hardware real.
Ver [relatório de mounts/interoperabilidade](fase-4-mounts-interoperabilidade.md)
e os logs `mounts-ahci-final-*` em `logs/`.

### Correção — revalidar armazenamento em mídia descartável — 16/11/2026

Sintoma: os resultados anteriores não eram da última árvore com o caso negativo
de overflow ELF. Causa: a suíte de conversões recebeu um caso novo e exigiu
rebuild. Repeti o smoke do bloco em ISO `storage-test` atualizada; a escrita
permaneceu restrita à imagem raw temporária QEMU.

Comando local: `CARLIO_STORAGE_TEST=1 scripts/build-iso.sh` seguido de
`TMPDIR=/tmp/carlioos-phase2-final-storage-20261116 python3
scripts/test-ahci-qemu.py --iso build/carlio-os.iso --timeout 60 --log
reports/logs/fase2-final-storage-qemu-local-20261116.log`. A imagem raw
temporária de 16 MiB passou IDENTIFY, Hermione formatar/gravar/remontar/ler/scrub,
WRITE/READ e erro ATA induzido. O temporário ficou em `/tmp/carlioos-*` e foi
removido pelo harness. Build: [ISO `storage-test`](logs/fase2-final-storage-build-20261116.log).

No NAS, `CARLIO_TEST_PHASE=fase2-final-20261116
CARLIO_TEST_QEMU_MEMORY=5G CARLIO_TEST_STORAGE=1 scripts/test-nas.sh` passou 160
testes host, BIOS/UEFI KVM e o mesmo teste Hermione/AHCI em imagem criada sob
`/tmp/carlioos-test-*/tmp`; o diretório de teste foi removido pelo cleanup do
script. Logs: [QEMU local](logs/fase2-final-storage-qemu-local-20261116.log),
[resumo NAS](logs/fase2-final-20261116-nas-20261116-192713.txt),
[BIOS NAS](logs/fase2-final-20261116-nas-20261116-192713-qemu-kvm-bios.log),
[UEFI NAS](logs/fase2-final-20261116-nas-20261116-192713-qemu-kvm-uefi.log) e
[Hermione/AHCI NAS](logs/fase2-final-20261116-nas-20261116-192713-filesystem-qemu-kvm.log).

Isto valida o caminho experimental em QEMU, não segurança para dados reais nem
um journal geral. Seguem abertos: corte de energia durante commit em QEMU,
timeout/remoção durante I/O, descoberta/seleção segura de partição e montagem
no boot, operações concorrentes e criptografia/KDF. Não houve acesso a discos
físicos.

Próximo gate: falha de energia QEMU injetada entre cada write/flush e recovery
após reinicialização; depois timeout e remoção virtual em I/O sob `storage-test`.
Registrar em `reports/logs/fase2-storage-powercut-*` e
`reports/logs/fase2-storage-timeout-*`; manter o escopo só em imagens descartáveis
sob `/tmp` e deixar “journal geral” sem alegação até o corte QEMU passar.

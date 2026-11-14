# Hermione — filesystem do CarlioOS

## Objetivo e compatibilidade

Hermione será um filesystem **escrito em Rust e inspirado nas propriedades do
ZFS**. A meta é copy-on-write, checksums ponta a ponta, snapshots, scrubs e
reparo com redundância. O formato em disco será próprio; não promete importar
ou montar pools ZFS existentes. Isso preserva a implementação Rust do sistema e
evita declarar compatibilidade antes de existir uma implementação completa.
O utilitário de linha de comando para administrá-lo se chama `herm-fs`.

Se interoperabilidade com pools ZFS existentes tornar-se obrigatória, será
necessária outra decisão de arquitetura: integrar OpenZFS como componente
externo ou planejar uma implementação Rust compatível com o formato e suas
feature flags. Leitura de metadados não basta para criar, atualizar, reparar e
validar um pool com segurança.

## Utilitário de linha de comando

O executável `herm-fs` é a interface de administração do Hermione. `--help` e
`status` informam a disponibilidade do formato; não há ainda interface para
administrar um volume real pelo CLI. O kernel tem um comando `fs-teste` somente
na build `storage-test`, para formatar e testar uma mídia descartável QEMU.

## Formato experimental v2

- `BlockDevice` oferece setores de 512 bytes, contagem de setores e operações
  de leitura/escrita; o driver AHCI QEMU implementa essa interface na build de
  teste.
- O volume tem superbloco versionado no LBA 0, duas cópias alternadas do
  diretório de 12 entradas nos LBAs 1 e 2, e extents contíguos a partir do LBA
  3. Nomes são ASCII simples, sem caminhos; o tamanho por arquivo é de 32 bits.
- Cada atualização grava dados em espaço livre, executa `flush`, grava a cópia
  inativa do diretório com nova geração e executa `flush` novamente. `mount`
  escolhe a geração válida mais recente ou recua para a cópia anterior se a
  outra estiver corrompida/incompleta. Superbloco e diretórios têm CRC32; cada
  arquivo tem CRC32 sobre bytes lógicos.
- `read_file` verifica o checksum antes de copiar os bytes para o buffer do
  chamador. `scrub` verifica todos os arquivos sem expor dados. A alocação
  first-fit percorre os extents registrados, evita a busca por cada setor e
  preserva o extent antigo até o commit de uma substituição.
- Montagem rejeita versão incompatível, cabeçalho inválido, setores fora da
  mídia e extents sobrepostos. A versão v2 não monta volumes experimentais v1;
  eles devem ser reformatados, sempre em mídia descartável.
- O teste host cobre formatação, escrita, remontagem, leitura, checksum,
  exclusão/reuso de espaço e nomes/buffers inválidos. A build `storage-test`
  ainda permite `fs-teste` formatar a mídia AHCI e remontar o volume durante a
  mesma sessão QEMU.
- **Ainda não é seguro para dados reais:** os diretórios duplos recuperam
  commits de metadados interrompidos em teste, mas não substituem um journal
  geral nem testes de corte de energia em hardware. Também faltam cifra, KDF,
  autenticação, árvore de diretórios, permissões, concorrência e montagem no
  boot normal. Reformatar sobrescreve os LBAs 0–2. Use somente disco descartável.

## Próximas etapas

- **Journal transacional geral:** manter recuperação explícita de operações
  multi-setor e confirmar as garantias de flush/torn-write em QEMU e dispositivos
  reais somente depois de existir suporte seguro a eles.
- **Integridade ponta a ponta:** checksums em dados e metadados detectam
  corrupção acidental; autenticação contra adulteração depende de MAC/AEAD e
  chaves protegidas. São garantias distintas e ambas serão verificadas.
- **Snapshots:** raízes imutáveis para restauração de versões; snapshot não é
  backup e não protege contra falha do mesmo conjunto de discos.
- **Scrub periódico:** varrer blocos em segundo plano, registrar progresso e
  erros, retomar após reinicialização e reduzir prioridade durante carga alta.
  A frequência será configurável e terá valor padrão documentado.
- **Autorreparo condicionado à redundância:** se uma cópia falhar no checksum,
  comparar uma cópia redundante validada (espelho/paridade), devolver os dados
  bons e regravar a cópia danificada. Sem outra cópia íntegra, detectar e
  identificar o erro, preservar o que restou e solicitar restauração; não
  inventar nem mascarar dados. Esse limite também se aplica ao ZFS.
- **Criptografia:** camada de volume desbloqueada no boot, com chaves fora do
  banco SQLite e segregadas de checksums. Checksum detecta corrupção, cifra
  autenticada detecta adulteração; são verificações diferentes.
- **SQLite:** arquivo/serviço de sistema no filesystem, submetido às mesmas
  transações e snapshots, mas nunca usado para armazenar chaves de volume.

## Testes obrigatórios

Injeção de corrupção em dados e metadados; perda de energia entre gravação e
commit; senha incorreta e adulteração; cópia redundante válida/inválida;
autorrecuperação e caso sem reparo possível; scrub com interrupção/retomada;
restauração de snapshot; benchmarks em disco único e espelho no QEMU/KVM. O
teste completo roda localmente e no NAS conforme `scripts/test-nas.sh`, com log
anexado ao relatório da fase 2.

## Referências de projeto

- [AHCI 1.3.1 — especificação Intel](https://www.intel.com/content/dam/www/public/us/en/documents/technical-specifications/serial-ata-ahci-spec-rev1-3-1.pdf)
- [Tratamento de erros SATA no guia libata do kernel Linux](https://docs.kernel.org/driver-api/libata.html)

- [Checksums no OpenZFS](https://openzfs.github.io/openzfs-docs/Basic%20Concepts/Data%20Storage/Checksums.html)
- [Scrub, resilver e autorreparo no OpenZFS](https://openzfs.github.io/openzfs-docs/Basic%20Concepts/Operations/Scrub%20and%20Resilver.html)
- [Copy-on-write no OpenZFS](https://openzfs.github.io/openzfs-docs/Basic%20Concepts/Copy-on-write.html)

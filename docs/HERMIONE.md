# Hermione — protótipo arquivado

**Estado do projeto:** desenvolvimento arquivado. ext4 é o filesystem principal
planejado e FAT32 atende EFI/mídia removível; Hermione não é o caminho de rootfs,
persistência ou instalação do produto. O código, o CLI e os relatórios históricos
permanecem preservados para referência, mas não recebem novos gates de produto.

## Objetivo e compatibilidade

Hermione foi um filesystem experimental **escrito em Rust e inspirado nas propriedades do
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

O executável `herm-fs` administra arquivos-imagem: `criar` usa criação
exclusiva e recusa sobrescrever, `listar`, `gravar`, `ler` e `scrub` montam a
imagem via um `BlockDevice` de arquivo com `sync_all` no `flush`. Arquivos de
entrada têm limite operacional de 64 MiB por chamada. O kernel tem `fs-teste`
somente na build `storage-test`, para validar a mesma API por AHCI em imagem
descartável QEMU; isso não monta volume no boot normal.

## Formato experimental v3

- `BlockDevice` oferece setores de 512 bytes, contagem, leitura, escrita e
  `flush`; o adaptador AHCI QEMU implementa a interface na build de teste.
- Superbloco no LBA 0; diretórios alternados nos LBAs 1 e 2; journal redo no
  LBA 3; dados a partir do LBA 4. Cada arquivo tem duas cópias contíguas,
  checksum CRC32 e tamanho máximo de 32 bits.
- Capacidade do dispositivo: exatamente até 2 TiB, isto é, 2^32 setores de 512
  bytes. O superbloco guarda a contagem em 64 bits; extents usam LBA de 32 bits
  e podem endereçar o último setor (`2^32-1`). Teste host com imagem esparsa
  grava/lê o último LBA e rejeita um setor após o fim. Isso não comprova AHCI
  físico nem liberação para dados reais.
- Commit grava os dois extents, faz `flush`, grava no journal a imagem completa
  do diretório com geração/CRC e faz `flush`, publica o diretório alternado e faz
  `flush`, então limpa o journal e sincroniza novamente. `mount` reaplica um
  journal válido mais novo que os diretórios.
- `read_file` confere as duas cópias antes de entregar conteúdo. `scrub` verifica
  cada cópia e, quando só uma diverge, restaura a cópia ruim usando a validada,
  sincroniza e relê para confirmar. Se ambas falharem, retorna erro sem inventar
  conteúdo. `ScrubReport` informa arquivos verificados e cópias reparadas.
- A alocação first-fit reserva os dois extents e preserva os dados anteriores
  até o commit. Montagem valida setores e sobreposição dos espelhos.
- A versão v3 é incompatível com v2 e exige reformatar somente uma imagem
  descartável. Testes host cobrem montagem persistente, cortes injetados em cada
  escrita/flush, replay de journal, corrupção simples/dupla, reparo e scrub.
  `storage-test` testa AHCI com imagem descartável no QEMU.
- **Ainda não é seguro para dados reais:** `flush` não foi validado em
  controladores físicos. O reparo cobre somente uma cópia corrompida com outra
  íntegra; não há paridade distribuída, cifra, permissões, concorrência nem
  montagem no boot normal. Use somente mídia descartável.

## Plano antigo (arquivado)

- **Journal transacional geral:** o redo de metadados v3 recupera publicação
  interrompida; ampliar operações multi-setor e validar durabilidade em
  controladores após suporte seguro a eles.
- **Integridade ponta a ponta:** checksums em dados e metadados detectam
  corrupção acidental; autenticação contra adulteração depende de MAC/AEAD e
  chaves protegidas. São garantias distintas e ambas serão verificadas.
- **Snapshots:** raízes imutáveis para restauração de versões; snapshot não é
  backup e não protege contra falha do mesmo conjunto de discos.
- **Scrub periódico:** varrer blocos em segundo plano, registrar progresso e
  erros, retomar após reinicialização e reduzir prioridade durante carga alta.
  A frequência será configurável e terá valor padrão documentado.
- **Autorreparo além do espelho local:** expandir para redundância validada e
  reconstrução com progresso persistente. Sem cópia íntegra, preservar o que
  restou e solicitar restauração, sem inventar nem mascarar dados.
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

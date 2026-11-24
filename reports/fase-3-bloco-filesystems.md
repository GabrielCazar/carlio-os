# Relatório — Fase 3: bloco e filesystems

### Aceite do bloco 3 em VM — 24/11/2026

- **Bloco e AHCI.** O NAS acessou o último LBA de um disco descartável de
  2 TiB e rejeitou leitura e escrita além dele. Em BIOS e UEFI, o QEMU de
  diagnóstico retirou o disco SATA com `PxCI` pendente e o HBA presente.
  O driver retornou `Removed`, preservou o buffer e recusou escrita e
  flush. Em outro cenário, retornou `Timeout`, preservou o buffer, emitiu
  COMRESET e concluiu uma nova leitura. [Capacidade](logs/fase3-aceite-nas/ahci-2tib.log) ·
  [Retirada BIOS](logs/fase3-aceite-nas/ahci-removal-bios.log) ·
  [Retirada UEFI](logs/fase3-aceite-nas/ahci-removal-uefi.log) ·
  [Timeout BIOS](logs/fase3-aceite-nas/ahci-timeout-bios.log) ·
  [Timeout UEFI](logs/fase3-aceite-nas/ahci-timeout-uefi.log).
  Os quatro cenários também passaram em [QEMU/TCG local](logs/fase3-aceite-local/).
- **Montagem e syscalls.** Em BIOS e UEFI no NAS, o kernel montou ext4 e
  executou um ELF do disco em CPL3, e montou FAT32 e leu `/probe.bin`
  por `openat/read/close`. [ext4 BIOS](logs/fase3-aceite-nas/exec-ext4-bios.log) ·
  [ext4 UEFI](logs/fase3-aceite-nas/exec-ext4-uefi.log) ·
  [FAT32 BIOS](logs/fase3-aceite-nas/leitura-fat32-bios.log) ·
  [FAT32 UEFI](logs/fase3-aceite-nas/leitura-fat32-uefi.log).
- **Persistência e recuperação.** Os testes em BIOS e UEFI repetiram o
  replay nativo dos inodes ext4 e entradas FAT32, a verificação das imagens
  por `e2fsck`/`fsck.fat` e a recuperação de dados e permissões do
  overlay VFS. [Logs](logs/fase3-aceite-nas/persistencia/).
- **Regressão.** 199 testes Cargo e Clippy do host aprovados. A ISO padrão
  iniciou em BIOS e UEFI no NAS. [Host](logs/fase3-aceite-local/host.log) ·
  [Clippy](logs/fase3-aceite-local/clippy.log) ·
  [BIOS](logs/fase3-aceite-nas/iso-bios.log) · [UEFI](logs/fase3-aceite-nas/iso-uefi.log).

A escrita nativa exercitada altera metadados e dados de arquivos existentes
em imagens dedicadas. As montagens normais ext4/FAT32 seguem em leitura.
A retirada foi validada com o comando submetido antes do DMA na VM.
A retirada elétrica em equipamento físico permanece pendente.


### Reprodução de timeout e retirada do disco SATA

O diagnóstico usa QEMU 11.1.2 com o [patch de teste](../scripts/qemu-11.1.2-ahci-test.patch).
O patch acrescenta um comando QMP de retenção para a porta SATA 0 do HBA
de teste. O guest escreve `PxCI` e o emulador mantém o slot pendente antes
de iniciar o DMA. Em um cenário, `device_del` só aceita a retirada quando
esse slot está pendente. O harness exige `DEVICE_DELETED`, link SATA caído
e HBA realizado. No outro cenário, a retenção só pode ser liberada depois
de o emulador observar COMRESET. Esse teste usa 100 mil iterações de polling
na leitura retida para tornar a execução prática em QEMU. O driver usa o
mesmo caminho de tratamento de timeout da ISO padrão.

Na extração do [QEMU 11.1.2](https://download.qemu.org/qemu-11.1.2.tar.xz),
aplicar o patch e compilar `x86_64-softmmu` com TCG e KVM. Rodar o diagnóstico
em uma ISO com `CARLIO_STORAGE_TEST=1` e usar o diretório de firmware do QEMU
da distribuição. Os dois comandos criam discos temporários descartáveis.

```bash
python3 scripts/test-ahci-qemu.py --iso build/carlio-os.iso \
  --qemu /caminho/qemu-system-x86_64 --qemu-data /usr/share/qemu \
  --remove-disk --hold-command --timeout 90 --log /tmp/carlioos-retirada.log
python3 scripts/test-ahci-qemu.py --iso build/carlio-os.iso \
  --qemu /caminho/qemu-system-x86_64 --qemu-data /usr/share/qemu \
  --command-timeout --timeout 90 --log /tmp/carlioos-timeout.log
```

Usar `--firmware /usr/share/edk2/x64/OVMF_CODE.4m.fd` para UEFI e
`--accel kvm` no NAS. Todo o I/O de teste remoto fica sob o diretório
temporário da execução.

Para repetir a leitura FAT32 por syscall, construir com
`CARLIO_DISK_READ_TEST=1` e executar `scripts/test-disk-exec-qemu.sh`
com `CARLIO_TEST_EXEC_FS=fat32`. A imagem de teste é criada com
`mkfs.fat` e `mcopy`. Para repetir o execve ext4, construir com
`CARLIO_DISK_EXEC_TEST=1` e usar o mesmo script com a seleção padrão.

### Capacidade AHCI e validação no NAS — 24/11/2026

O teste AHCI usa o adaptador `BlockDevice` para gravar padrões distintos no LBA
1 e nos dois últimos setores, executar flush e reler os três setores. Também
confirma que leitura e escrita imediatamente além do disco retornam
`OutOfRange`. O NAS Manjaro executou a ISO pelo QEMU/KVM em BIOS e UEFI com
um QCOW2 esparso de 2 TiB em `/tmp`. A prova de escrita e releitura sob BIOS
confirmou o último LBA (`2^32-1`) no NAS/KVM e em QEMU/TCG local.
Os 198 testes Cargo passaram na estação.

O NAS está acessível pelo alias cabeado `nas-manjaro`. A conta `gabriel`
tem Rust nightly, GCC e as dependências do `Cargo.lock` em uma instalação
offline no diretório pessoal. A execução completa rodou os 198 testes Cargo
no NAS, iniciou a ISO construída na estação em BIOS e UEFI e repetiu a
prova AHCI de 2 TiB no KVM.

[Execução completa](logs/fase-3-nas-completo-nas-20261124-131925.txt) ·
[AHCI/KVM de 2 TiB](logs/fase-3-nas-completo-nas-20261124-131925-filesystem-qemu-kvm.log).

[BIOS/KVM](logs/fase-3-capacidade-nas-20261124-131517-qemu-kvm-bios.log) ·
[UEFI/KVM](logs/fase-3-capacidade-nas-20261124-131517-qemu-kvm-uefi.log) ·
[AHCI/KVM](logs/fase-3-capacidade-nas-20261124-131517-filesystem-qemu-kvm.log) ·
[AHCI/TCG](logs/fase3-capacidade-qemu-local-20261124.log).

### Swap, persistência e integridade — 24/11/2026

- `SwapStore` usa uma região de bloco delimitada e slots de 4 KiB. As páginas
  são cifradas e autenticadas com XChaCha20-Poly1305. O nonce combina sessão
  e contador. O slot e o proprietário fazem parte dos dados autenticados.
  O identificador só é publicado após o flush. Na leitura, erro de I/O ou
  autenticação mantém o slot disponível para tratamento e limpa o destino.
  Chave e buffers temporários usam limpeza explícita. A chave e o
  identificador de sessão são fornecidos pelo chamador.
- `PersistentLog` grava registros de até 490 bytes em um anel. Cada posição
  tem dois setores alternados e cada cópia tem sequência e CRC32. O flush
  precede o reconhecimento do append. Em erro ambíguo, a região precisa ser
  reaberta para recuperar o último registro válido. O CRC32 detecta
  corrupção acidental. A abertura lê a região inteira.
- QEMU com `storage-test` confirmou page-out/page-in cifrado via AHCI e
  releitura do registro em uma segunda VM com a mesma imagem descartável.
  [BIOS gravação](logs/fase3-persistencia-bios-gravar.log) ·
  [BIOS leitura](logs/fase3-persistencia-bios-ler.log) ·
  [UEFI gravação](logs/fase3-persistencia-uefi-gravar.log) ·
  [UEFI leitura](logs/fase3-persistencia-uefi-ler.log).
- Os 197 testes de host incluem escrita parcial, falha de flush, corrupção
  das duas cópias do log, slot reutilizado, proprietário incorreto e
  adulteração do ciphertext. [Host](logs/fase3-persistencia-host-20261124.txt) ·
  [Clippy](logs/fase3-persistencia-clippy-20261124.txt).

O kernel ativa o coletor em um disco AHCI dedicado com o primeiro setor
igual à assinatura ASCII `CARLIO-PERSIST-01` seguida de zeros. O disco deve
ter pelo menos 4104 setores. O anel de erros ocupa os LBAs 2064–2191
e a reserva de swap começa no LBA 4096, em páginas de oito setores.
O instalador ainda precisa criar ou selecionar essa mídia.

O coletor enfileira até 16 eventos recentes em RAM e grava no contexto
do shell. A coleta é de melhor esforço. Uma falha de I/O suspende a escrita
até o próximo boot. Em mídia provisionada, o boot gera chave e sessão a
partir de RDRAND com tentativas limitadas. O teste AHCI consome essas
credenciais. A edição padrão descarta a chave até a integração com a VM.

O cenário em imagem descartável simula um setor parcial no caminho AHCI,
encerra a VM com SIGKILL antes do flush do log, confere o setor na imagem
bruta e remonta para validar a cópia anterior e o evento do coletor.
O teste usa `cache=directsync` para tornar a escrita parcial reproduzível.
Passou em BIOS e UEFI, junto com 198 testes de host e Clippy.
[BIOS corte](logs/fase3-reserva-bios-corte.log) ·
[BIOS recuperação](logs/fase3-reserva-bios-ler.log) ·
[UEFI corte](logs/fase3-reserva-uefi-corte.log) ·
[UEFI recuperação](logs/fase3-reserva-uefi-ler.log).

### Recuperação de dados ext4/FAT32 após corte — 24/11/2026

O teste de armazenamento sobrescreve um setor de um arquivo preexistente em
uma imagem ext4 ou FAT32 descartável. O harness encerra a VM no ponto de
escrita parcial, verifica o registro redo e os bytes parciais na imagem e
reinicia a VM. O kernel recupera o setor e confirma que repetir a recuperação
é idempotente. Os quatro cenários passaram em BIOS e UEFI, no QEMU/TCG local
e no NAS/KVM.

[ext4 BIOS](logs/fase3-recuperacao-nas-20261124/fase3-ext4-bios-recuperacao.log) ·
[ext4 UEFI](logs/fase3-recuperacao-nas-20261124/fase3-ext4-uefi-recuperacao.log) ·
[FAT32 BIOS](logs/fase3-recuperacao-nas-20261124/fase3-fat32-bios-recuperacao.log) ·
[FAT32 UEFI](logs/fase3-recuperacao-nas-20261124/fase3-fat32-uefi-recuperacao.log).

### Recuperação de metadados ext4/FAT32 — 24/11/2026

O diagnóstico altera os bits de permissão de um inode regular ext4 de
`0644` para `0640` e recalcula o CRC32C do inode. No FAT32, altera o
atributo somente leitura da entrada de diretório. Cada operação grava o
setor completo de metadados e publica um registro redo em dois setores
reservados fora da partição. As montagens VFS continuam em leitura para
os dois formatos. Alocação, criação, remoção e replay JBD2 ficam fora
deste incremento.

O teste grava parcialmente o setor de destino após o flush do redo,
zera a metade que contém o campo alterado e encerra o QEMU. No boot
seguinte, verifica a recuperação do setor, os atributos e o conteúdo
do arquivo. O segundo replay é idempotente. Os dois formatos passaram
em BIOS e UEFI, em TCG local e KVM no NAS. `e2fsck -fn` e
`fsck.fat -vn` verificaram as imagens recuperadas.
O teste host também rejeita tipo de registro, destino e CRC inválidos.
Os 199 testes e o Clippy do host passaram. O Clippy com alvo bare metal
e avisos tratados como erros ainda aponta diagnósticos em código fora
deste incremento.

[Clippy bare metal](logs/fase3-metadados-local/clippy-kernel.log) ·
[Logs QEMU/TCG](logs/fase3-metadados-local/) ·
[Logs NAS/KVM](logs/fase3-metadados-nas/) ·
[Testes host](logs/fase3-metadados-local/host.log).

### Perda de acesso durante leitura AHCI — 24/11/2026

O driver verifica o link antes de submeter o comando, durante o polling e
antes de publicar a conclusão. A perda do link invalida o dispositivo até
o próximo boot. Uma transferência curta retorna erro e o timeout aciona
COMRESET antes de permitir novo I/O. Se o reset falha, o driver bloqueia
comandos subsequentes.

QEMU retirou um controlador AHCI dedicado depois da submissão de uma leitura
e antes de o driver consumir sua conclusão. A leitura devolveu `Removed`
com o buffer intacto, e o mesmo adaptador recusou escrita e flush. A prova
passou em TCG local e KVM no NAS.

[QEMU local](logs/fase3-ahci-remocao-local-20261124.log) ·
[NAS/KVM](logs/fase-3-remocao-nas-20261124-132432-ahci-remocao-qemu-kvm.log).

O cenário `scripts/test-ahci-qemu.py --remove-disk` envia `device_del`
ao disco SATA após a submissão da leitura e exige confirmação de remoção
com o HBA ainda presente. O QEMU de distribuição rejeitou a operação no
barramento IDE do ICH9 AHCI. O patch de diagnóstico descrito acima habilita
o cenário em uma porta SATA de teste.

[Tentativa local](logs/fase3-metadados-local/fase3-sata-remover-disco.log) ·
[Tentativa no NAS](logs/fase3-metadados-nas/fase3-sata-remover-disco.log).


### Overlay persistente do VFS — 24/11/2026

Uma imagem dedicada provisionada com `scripts/provision_storage.py --vfs`
habilita persistência dos dados e metadados da árvore VFS. O layout de versão
2 reserva os LBAs 4096–8191 para o overlay e inicia o swap no LBA 8192.
O layout anterior continua com swap no LBA 4096.

As mutações do VFS serializam a árvore no banco inativo, descarregam os dados
e publicam um cabeçalho com geração e checksums. O boot seleciona a geração
válida mais recente. Se a gravação falhar, o kernel reverte a alteração em
memória, devolve erro e suspende novas mutações do overlay até o próximo boot.
A capacidade continua limitada a 40 nós e 16 KiB por arquivo.

O teste gravou um arquivo com modo `0640`, confirmou a leitura após reboot,
publicou outra geração com novo conteúdo e modo `0600`, corrompeu o cabeçalho
mais recente e verificou a recuperação do conteúdo e modo anteriores.
O teste exercita a transação VFS pelo shell de diagnóstico.

[BIOS local](logs/fase3-vfs-bios-recuperar.log) ·
[UEFI local](logs/fase3-vfs-uefi-recuperar.log) ·
[BIOS NAS/KVM](logs/fase3-vfs-nas/fase3-vfs-bios-recuperar.log) ·
[UEFI NAS/KVM](logs/fase3-vfs-nas/fase3-vfs-uefi-recuperar.log).

### Correção — nomes longos FAT32 e seleção da FAT ativa — 24/11/2026

O leitor FAT32 agora aceita nomes longos VFAT na resolução de caminhos. As
entradas UTF-16 são vinculadas ao alias 8.3 por ordem e checksum. Uma sequência
inválida preserva o acesso pelo alias. A comparação ignora caixa para ASCII
e compara os demais caracteres pelo valor Unicode. A listagem de raiz
continua expondo aliases 8.3.

Quando o espelhamento está desabilitado, o leitor usa a cópia selecionada
pelo BPB. O fim válido de cadeia encerra a listagem, enquanto clusters livres,
reservados ou defeituosos retornam erro de cadeia corrompida. As alterações
ficam em `src/fat32.rs` e a montagem continua somente leitura.

- [Host](logs/fase3-fat32-host-20261124.txt) · [Clippy](logs/fase3-fat32-clippy-20261124.txt).
- [QEMU BIOS](logs/fase3-fat32-qemu-bios-20261124.log) · [QEMU UEFI](logs/fase3-fat32-qemu-uefi-20261124.log). A imagem MBR descartável exercitou descoberta e montagem. Os casos de nome longo, alias e cadeia corrompida foram exercitados no host.

NAS/KVM e hardware físico aguardam validação. Escrita ext4/FAT32, remoção durante I/O e recuperação dos filesystems seguem
abertas.


### Correção — regressão de storage e enforcement CPL3 — 2026-11-16

Repetimos `storage-test` com AHCI QEMU `8086:2922` em imagem raw descartável
de 16 MiB, tanto em TCG local como em KVM no NAS. Ambos confirmaram IDENTIFY,
WRITE/READ, comando ATA inválido, COMRESET/reuso do controlador e ciclo
Hermione de formatação, gravação, remontagem, leitura, checksum e scrub. A
suíte NAS executou 156 testes host e boots BIOS/UEFI; o probe CPL3 de segurança
também registrou negação `EACCES` para arquivo `0600` de outro UID.

Evidências locais: [build storage-test](logs/fase-3-storage-build-20261116.txt),
[AHCI/serial](logs/fase-3-storage-qemu-local-20261116.log) e
[resultado do harness](logs/fase-3-storage-qemu-local-20261116.txt). Evidências
NAS/KVM: [suíte host e boot](logs/fase-3-seguranca-storage-20261116-nas-20261116-194926.txt),
[BIOS](logs/fase-3-seguranca-storage-20261116-nas-20261116-194926-qemu-kvm-bios.log),
[UEFI](logs/fase-3-seguranca-storage-20261116-nas-20261116-194926-qemu-kvm-uefi.log)
e [AHCI/Hermione](logs/fase-3-seguranca-storage-20261116-nas-20261116-194926-filesystem-qemu-kvm.log).

**Estado:** validado novamente somente em mídia virtual descartável. Timeout e
remoção durante I/O, corte de energia QEMU durante commit, montagem no boot,
controladores físicos, FAT32/ext4 graváveis e criptografia continuam abertos;
não declarar journal geral nem segurança para dados reais.

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

- Boot físico, drivers RTL8111/I219-LM e retirada de disco SATA em
  equipamento físico continuam pendentes.
- A política de montagem gravável e as operações nativas de alocação,
  criação e remoção de arquivos continuam abertas.
- O layout dedicado atual reserva 64 KiB para o coletor e páginas de swap a
  partir do LBA 4096. A meta de dimensionamento de 16 GiB e a integração ao
  instalador e à paginação seguem abertas.
- Hermione permanece formato experimental incompatível com v2 e não deve
  receber dados reais. A montagem normal, cifra, árvore de diretórios e
  redundância além do espelho local seguem pendentes.

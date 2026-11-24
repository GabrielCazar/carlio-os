# Implementações locais e estado da validação

O inventário abaixo registra o estado histórico de novembro de 2026. A
[revalidação de 23/11](#revalidação-local--2026-11-23) registra os resultados novos.

Estado: implementação local em andamento, sem validação desta rodada. O pedido
atual prioriza itens que dependam de confirmação do usuário; não foi encontrado
um bloqueio de produto que exija decisão para continuar o trabalho local. Testes
em hardware físico e qualquer escrita em disco continuam fora de escopo sem
autorização e gates próprios. Nenhuma funcionalidade listada aqui deve ser
anunciada como validada ou suportada até os gates correspondentes passarem.

## Fase 2 — bundle do processo

Commit já criado: `1953b57 fix(process): associa bundle de recursos à tarefa`.
O `Process` recebe manifesto com slot de VM, CR3 e stack user; syscall e reaper
usam o mesmo token. Evidência executada antes da suspensão: host e QEMU local
BIOS/UEFI passaram; NAS BIOS passou; NAS UEFI falhou em preempção/page tables,
como registrado no relatório da Fase 2. O objeto `ProcessVm` ainda reside em
registro fixo do backend. Pendências: ownership direto, contabilidade de
memória por processo, stress sustentado scheduler/VM/allocator e gate NAS UEFI.

## ABI Linux e libc freestanding — implementação sem gates

- `mprotect(10)` foi conectado à ABI, a `ProcessVm` e ao backend da fixture.
  Escopo inicial: intervalo dentro de uma única VMA anônima; rejeita W+X,
  informa `EOPNOTSUPP` para `PROT_NONE`, atualiza permissões de página e tenta
  restaurar permissões anteriores se uma alteração falhar. A fixture CPL3 foi
  estendida para gravar numa página, torná-la read-only e verificar flags antes
  do unmap. Intervalos que cruzem VMAs diferentes e mapas não anônimos não estão
  incluídos.
- A libc local expõe `mprotect`, `clock_gettime`, `clock_getres`, `gettid`,
  `getuid`, `geteuid`, `getgid`, `getegid`, `getgroups` e `brk` com `ENOMEM`
  quando o retorno do kernel não alcança o endereço pedido; também inclui
  `uname` e layout `utsname` x86_64 de seis campos com 65 bytes.
- Foi acrescentado `sbrk(long)` na libc freestanding. Obtém o break atual via
  `brk(0)`, calcula incrementos positivos/negativos sem overflow aritmético e
  devolve o break anterior; não há proteção entre threads e o wrapper não foi
  compilado nem exercitado.
- `clock_getres(id, NULL)` agora retorna sucesso sem escrever memória, como a
  ABI Linux permite; `clock_gettime` continua exigindo buffer de saída. A
  semântica ainda não foi executada contra um binário userland.
- Foram conectados `fstat(5)` e `newfstatat(262)` ao dispatcher e à metadata do
  VFS inline. A saída usa layout Linux x86-64 de 144 bytes, valida endereço não
  nulo/alinhado, inclui tipo/permissões, inode de slot VFS, uid/gid, tamanho e
  blocos; timestamps ficam zero. `newfstatat` usa cwd `/`, ignora dirfd em
  caminhos absolutos, aceita `AT_FDCWD` para relativos e só aceita flags zero.
  A libc declara `fstat`, `stat`, `fstatat` e layout em `sys/stat.h`. Permanecem
  fora stdin/stdout/stderr, dirfds relativos, links, flags `AT_*`, timestamps
  reais e estabilidade de inode após reuso de slot; nada
  foi compilado ou exercitado.
- Foi conectado `access(21)` usando `AccessContext` e a política DAC já aplicada
  pelo VFS, validando apenas `F_OK/R_OK/W_OK/X_OK` e combinações válidas sob
  cwd `/`. O modelo atual usa as credenciais disponíveis no processo
  (sem distinção entre IDs reais e efetivos); wrapper e constantes da libc foram
  adicionados. Sem gates ou prova CPL3 nesta suspensão.
- `getcwd(79)` agora copia `/` com NUL, retorna comprimento 2 no kernel e a libc
  converte sucesso para o ponteiro do buffer. A operação retorna `ERANGE` para
  buffers menores que dois bytes; o cwd permanece fixo porque `chdir` ainda não
  existe. Não compilado nem exercitado.
- `mkdir(83)` foi conectado ao dispatcher Linux x86-64, à criação de diretórios
  do VFS inline sob o `AccessContext` do processo e à persistência do rootfs; a
  libc declara o wrapper em `sys/stat.h`. Modo acima de 16 bits retorna `EINVAL`
  e o VFS aplica a `umask` já guardada no processo. `EEXIST` agora é mapeado
  explicitamente. `umask(95)` foi conectado para ler/atualizar esse contexto e
  tem wrapper libc em `sys/stat.h`; ambas as chamadas aguardam validação. Se a
  gravação do snapshot falhar após mutação, o VFS em memória permanece alterado.
- `chmod(90)` foi ligado ao método `set_mode` do VFS, que limita permissões
  suportadas, resolve com os grupos suplementares do processo e exige
  proprietário ou root; alterações persistem no rootfs e a
  libc declara o wrapper em `sys/stat.h`. Caminho relativo usa cwd `/`; modo
  acima de 16 bits retorna `EINVAL`. Nenhuma validação foi executada.
- `fchmod(91)` usa o descritor VFS aberto para obter o caminho, delega à mesma
  checagem de proprietário/root e persiste o modo; a libc declara o wrapper em
  `sys/stat.h`. Como em `chmod`, falha de persistência não restaura o modo em
  memória. Sem validação runtime.
- `chown(92)`, `fchown(93)` e `lchown(94)` foram ligados ao VFS e à libc; os
  sentinelas UID/GID `u32::MAX` preservam o campo existente, a operação exige
  root e mudança efetiva de proprietário/grupo limpa bits setuid/setgid. Não há
  links simbólicos; persistência e semântica de descritor aguardam validação.
- `fchownat(260)` e `fchmodat(268)` foram ligados para `AT_FDCWD` ou caminho
  absoluto e flags zero/`AT_SYMLINK_NOFOLLOW` (sem links no VFS); dirfds reais
  para caminhos relativos e demais flags retornam erro. Wrappers e declarações
  libc adicionados; nenhuma validação executada.
- `unlink(87)` e `unlinkat(263)` removem arquivos via permissões de diretório
  do VFS e persistem o snapshot. `unlinkat` aceita flags zero e dirfd relativo
  somente em `AT_FDCWD`; caminho ainda aberto retorna `EBUSY` porque os descritores
  guardam caminho, não referência de inode. Diretórios continuam recusados.
  Wrappers libc adicionados. Falha ao persistir após remover deixa a árvore em
  memória alterada; nenhuma validação executada.
- `rmdir(84)` e `unlinkat(..., AT_REMOVEDIR)` removem somente diretórios vazios;
  o VFS aplica DAC/sticky bit e bloqueia pontos de mount. `ENOTDIR`, `ENOTEMPTY`
  e `EBUSY` receberam mapeamento. Wrapper libc e constante `AT_REMOVEDIR` nos
  headers `fcntl.h` e `sys/stat.h` foram adicionados. Persistência ainda não faz
  rollback da árvore em memória e o caminho aguarda validação.
- `openat` deixou de rejeitar `O_CREAT` e aceita também `O_EXCL`, usando a
  criação inline e persistência já implementadas no backend. O modo passa por
  `u16::try_from` antes de chegar ao VFS. Flags comuns ainda ausentes, como
  `O_CLOEXEC`, continuam explicitamente recusadas; gate CPL3/host suspenso.
- A tabela de fds agora permite reserva invisível (`ready=false`) e publicação
  explícita. O syscall reserva antes de consultar/criar/truncar no VFS, define o
  offset inicial e publica só após a persistência; em erro, fecha a reserva.
  Assim, tabela cheia não cria o arquivo antes de retornar `ENFILE`. Permanece
  sem validação de concorrência/runtime.
- `OpenFileTable` e `SocketTable` agora oferecem recolhimento de todas as
  entradas por PID; o reaper da fixture CPL3 executa esse cleanup após teardown
  de VM bem-sucedido. A ligação ao reaper de processos gerais ainda não existe,
  pois o backend só possui essa rotina de reap para a fixture. Sem validação.
- A serialização de mutações do rootfs passou de `IrqSpinLock` bloqueante para
  reserva atômica sem espera; contenção retorna `EAGAIN`. Snapshot e volume são
  retirados de seus spinlocks antes do I/O de bloco, e o volume é recolocado
  após sucesso ou erro. Conversões de tamanho/offset em append/write usam
  `try_from` e soma checked. Não houve build, teste ou execução deste caminho.
- `fsync(74)` agora exige fd aberto da tabela VFS e força a persistência do
  snapshot; a libc declara o wrapper. `fsync` e mutações do VFS compartilham a
  reserva atômica, sem spinlock durante I/O; fd de socket/std não é aceito.
  Falta validar falha de mídia e semântica de durabilidade.
- Os três VMs nomeados A/B/C da fixture foram substituídos por pool bounded de
  64 slots tipados `Option<RuntimeProcessVm>`, com reserva de slot livre e token
  (`u8`) gravado no `ProcessResourceBundle`. O rollback pré-dispatch e abort do
  processo agora localizam VM pela raiz esperada, sem pressupor `slot == índice
  da fixture`. O pool ainda é storage estático do backend (não ownership direto
  nem crescimento dinâmico do `Process`); nada foi compilado/executado.
- Falhas antes da associação ao scheduler agora tentam destruir cada VM; se o
  teardown falha, o objeto é retido em um slot livre para impedir retry que
  sobrescreva ownership. A fixture bloqueia nova execução quando encontra slots
  pendentes. Overflow do próprio pool ainda é caminho fail-closed não exercitado.
- `mmap_linux` substitui conversão estreita direta de proteção por
  `u8::try_from` após validar as flags.
- `OwnedAddressSpace`/`ProcessVm` agora expõem `ProcessMemoryUsage`, com folhas
  owned, tabelas privadas user alcançáveis (incluindo a raiz PML4), total owned
  e limites fixos de folhas e de frames totais (cota superior de `4 * folhas +
  1`, incluindo até três níveis de tabela por folha). Quota dinâmica ainda falta.
- A probe de boot registra folhas/tabelas/total/limites para os três espaços e
  exige baseline 2/4/6/128/513; o smoke espera esse marcador. A contagem percorre somente a
  metade user do PML4, excluindo tabelas compartilhadas do kernel. Contagens são
  impressas com conversão `usize`→`u64` validada e falha explícita em overflow.
- Foi adicionado teste host para contabilização 1 folha + 4 tabelas durante
  `mmap`, e 0 folhas + somente a raiz após `munmap`; permanece não executado.
- Testes adicionais foram escritos para splits VMA/permissões de `mprotect`,
  leitura preservada, W+X negado e `PROT_NONE` reportado como não suportado;
  permanecem não executados.
- Foi acrescentado teste de stress host no scheduler: quatro workers executam
  20.000 ciclos concorrentes de preempção local sobre filas isoladas, verificando
  que cada processo permanece na CPU proprietária. O teste está escrito, não foi
  compilado nem executado nesta suspensão; não prova CPL3 real nem VM/allocator.
- A fixture CPL3 de dois APs agora executa 512 ciclos de `mmap/munmap` por AP
  antes de aceitar a parada; o reaper probe registra os totais e só passa se
  ambos atingirem o alvo, confirmarem sobreposição e retornarem a idle. O código
  de boot não foi compilado nem executado; não há evidência runtime desta probe.
  O prazo de espera pelo stress foi ampliado para 1000 ticks finitos.
- O smoke QEMU passou a exigir as novas mensagens da probe mprotect e do stress
  por AP, para que validação futura não aceite apenas a mensagem de overlap.
- Contrato e documentação de userland/Arch foram atualizados para registrar o
  estado parcial e a suspensão de validação.
- Os retornos antecipados da criação da fixture CPL3 agora usam rollback dos
  processos ainda não despachados: transicionam processos Ready para encerrados,
  recolhem stacks/VMs quando a raiz confere e preservam os IDs e o baseline para
  retry se o cleanup falhar. Esse caminho cobre criação de credenciais,
  criação/associação/afinidade/frame inicial; não foi compilado nem executado.
  Se qualquer retry de cleanup deixar IDs ou slots ocupados, a fixture agora
  bloqueia uma nova rodada para não sobrescrever ownership pendente.

## Gates adiados

- `cargo fmt --check`, `cargo test --all-targets`, Clippy e build
  `x86_64-unknown-none`.
- Smoke QEMU local BIOS/UEFI, probes de permissões/W^X/rollback e de buffers
  user válidos/inválidos.
- NAS/KVM BIOS/UEFI, estritamente em `/tmp/carlioos-*`.
- Revisão de semântica Linux de `mprotect`, ABI/layout `timespec`, compatibilidade
  C da libc e relatório formal da fase antes do próximo commit.

Próximo trabalho: criar probes positivos, negativos e de falha para mprotect;
validar ABI dos wrappers C; integrar quotas de memória por processo e stress
concorrente de scheduler/VM/allocator.

## Verificação estrutural adicional — associação de VM

Revisão estática encontrou risco de um manifesto apontar para o slot de outro
VM: o CR3 declarado era comparado ao processo, mas não à raiz real no registro.
Agora o scheduler recusa slots já reivindicados por outro processo, o dispatcher
confere `ProcessVm::root()` antes de cada operação e o reaper só retira/destrói
um VM cuja raiz corresponde ao processo que está sendo recolhido. Foi ampliado
o teste host para a colisão entre dois processos; não foi compilado nem executado.

## Accounting de tabelas privadas — sem validação

O contador de memória agora percorre recursivamente as entradas user 0..255 do
PML4 privado e conta raiz, PDPT, PD e PT alcançáveis; o half kernel compartilhado
fica excluído. A fixture exige baseline `2 folhas / 4 tabelas / total 6 / limite
128 / teto 513` nos três processos e o smoke valida o marcador. Os gates continuam
suspensos; essa expectativa ainda precisa ser confirmada em host e QEMU BIOS/UEFI.

O reaper também captura frames livres antes de criar os espaços e compara a
contagem depois de recolher os três processos. Divergência impede o probe de
passar; o smoke espera mensagem positiva de retorno exato ao baseline. Nenhuma
execução runtime ocorreu durante a suspensão dos gates.

## Validação local — 2026-11-19

- `cargo fmt --check`: passou.
- `cargo test --all-targets`: 188 testes passaram em quatro suites.
- `cargo clippy --all-targets -- -D warnings`: passou sem avisos.
- `cargo build --release --bin carlio-kernel --target x86_64-unknown-none`:
  passou com um aviso de função não usada em `src/arch/x86_64/idt.rs`.
- O smoke QEMU BIOS iniciou o kernel, despachou tarefas CPL3 e parou após a
  mensagem de espera por entrada serial; os timeouts de 25 s e 100 s expiraram
  na etapa 1/7. Uma repetição estendida foi interrompida após continuar sem
  saída posterior. Assim, probes CPL3, UEFI e as mensagens finais do smoke não
  estão validadas nesta rodada; o script não chegou a tentar UEFI.
- O gate NAS/KVM executou 188 testes host com sucesso e construiu a ISO, mas o
  smoke BIOS falhou após `LAPIC: ACKs obtidos=2, esperados=3` e timeout do
  shootdown remoto. O script não chegou ao smoke UEFI. Relatórios e log serial:
  `reports/logs/kernel-context-switch-nas-20261119-160723.txt` e
  `reports/logs/kernel-context-switch-nas-20261119-160723-qemu-kvm-bios.log`.
- A ISO `build/carlio-os.iso` foi gravada em `/dev/sdb` (USB removível, Flash
  Disk, 7,8 GB). Leitura de volta com `cmp` confirmou que os 6.227.968 bytes da
  imagem correspondem ao conteúdo inicial do dispositivo. O USB foi escrito em
  modo de imagem híbrida, sem montagem de partições; o smoke de boot dessa cópia
  ainda não foi feito.
- A correção do retorno `usize` de `read` e do avanço `u64` de `write` está
  somente no workspace e ainda não foi commitada.

## Revalidação local — 2026-11-23

- Formatação, build do kernel, build da libc freestanding e 190 testes host passaram.
  [Testes host](logs/fechamento-andamento-host-20261123-230106.txt)
- QEMU TCG com quatro CPUs passou em BIOS e UEFI. O BSP preservou o CR3 do
  kernel, a barreira recebeu três ACKs e a contagem de frames voltou ao
  baseline. Cada um dos dois APs de teste completou 512 ciclos de mmap/munmap.
  [BIOS](logs/fechamento-andamento-qemu-bios-20261123-230106.log) ·
  [UEFI](logs/fechamento-andamento-qemu-uefi-20261123-230106.log)
- QEMU BIOS passou com dois discos descartáveis em modo snapshot, cada um com
  uma partição MBR. O VFS montou ext4 e FAT32 em `/` para leitura. O volume
  ext4 de teste foi formatado com `flex_bg`, `metadata_csum_seed` e
  `orphan_file` desabilitados. A prova CPL3 de negação POSIX passou com os
  dois volumes montados.
  [ext4](logs/fechamento-andamento-ext4-qemu-bios.log) ·
  [FAT32](logs/fechamento-andamento-fat32-qemu-bios.log)
- NAS/KVM ainda sem resultado. SSH retornou falta de rota por Wi-Fi e
  expirou pelo endereço cabeado. O Clippy com `-D warnings` segue falhando
  em `while_let_on_iterator` em `src/ext4.rs` e `result_unit_err` em
  `src/userland/kdf.rs`.
- Execução de `execve` a partir de um arquivo do disco, stress sustentado,
  persistência em disco, validação física e os gates das userlands completas
  permanecem em aberto.

### Clippy, SMP ampliado e nova tentativa no NAS

- `cargo clippy --all-targets -- -D warnings` passou após ajustar a iteração
  do leitor ext4 e tipar os erros da derivação Argon2id. Os 190 testes host
  passaram novamente.
- Em QEMU TCG, `CARLIO_SMP_STRESS_TEST=1 ./scripts/build-iso.sh` amplia de
  512 para 8.192 os ciclos de mmap/munmap em cada AP de teste. Para rodar o
  smoke da imagem, passe `--smp-cycles 8192 --timeout 40` a
  `scripts/qemu-smoke.py`. Os logs registram o retorno ao baseline de frames.
  A primeira execução UEFI expirou aguardando o byte serial; a repetição
  passou. [BIOS](logs/smp-stress-qemu-bios-20261123.log) ·
  [UEFI](logs/smp-stress-qemu-uefi-20261123.log)
- SSH continuou indisponível por Wi-Fi, cabo e pelo alias remoto. O endereço
  cabeado configurado no alias está fora das sub-redes da interface local.
  O vizinho detectado no enlace recusou conexão na porta SSH.
- A inspeção local não encontrou mídia removível para teste de boot físico.
  `execve` de disco, persistência e execução das userlands completas seguem
  abertos.

## Execução de ELF estático em ext4

- O probe opt-in chama `execve("/carlio-disk-exec", NULL, NULL)` por SYSCALL.
  O ELF mora em uma partição MBR/ext4 descartável. Sua entrada confere o
  `argc` da nova pilha, escreve o marcador via syscall e encerra em CPL3.
- O leitor ext4 preenche com zeros blocos esparsos e extents não inicializados.
  O carregador exige a leitura integral do arquivo executável montado e
  propaga erros de resolução e acesso do VFS.
- O registro de VMs usa aquisição sem espera para pular slots em uso por
  outros CPUs durante shootdown. A verificação de memória segue o slot
  registrado no processo depois da troca de imagem.
- As pilhas de kernel por tarefa passaram de 64 para 128 KiB. O watermark
  observou 78.856 bytes no teste; a contagem de frames após o reaper voltou
  exatamente ao baseline.
- O smoke QEMU TCG com quatro CPUs passou em
  [BIOS](logs/disk-exec-ext4-qemu-bios.log) e
  [UEFI](logs/disk-exec-ext4-qemu-uefi.log). Esta prova usa um ELF estático
  dentro do limite de 16 KiB por imagem do carregador atual. A ISO padrão
  recompilada também passou no smoke BIOS e UEFI.
- Formatação, 191 testes host e Clippy com `-D warnings` passaram.

Para reproduzir, execute `CARLIO_DISK_EXEC_TEST=1 ./scripts/build-iso.sh` e
`scripts/test-disk-exec-qemu.sh build/carlio-os.iso /tmp/disk-exec.log`.
Para UEFI, acrescente `--firmware` com o arquivo OVMF CODE correspondente.

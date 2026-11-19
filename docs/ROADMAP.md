# Roadmap do CarlioOS

## Propósito do produto

Entregar um sistema operacional estável e seguro para servidores/NAS x86_64,
com kernel CarlioOS implementado em Rust e userland compatível com Arch Linux e
Ubuntu (Debian packaging). A
meta inclui compatibilidade progressiva com pacotes Debian (`.deb`), RPM
(`.rpm`), executáveis Windows (`.exe`), AppImage, Flatpak/Flathub e Snap. A
console nativo será o zsh da userland Arch ou um shell escrito em Rust com
compatibilidade de comandos e plugins zsh validada por matriz. A arquitetura de
64 bits (CPU x86-64, kernel, endereços virtuais, processos, ELF e ABI) é um
requisito de ponta a ponta; nenhuma camada pode truncar endereços ou tamanhos
para 32 bits. O sistema deve
inicializar em BIOS e UEFI, oferecer armazenamento recuperável, rede e serviços
de servidor, suportar contas e permissões reais, e proteger discos e dados já
existentes durante instalação e atualização. Pacman/ALPM e aplicações Arch são
prioridade; compatibilidade com formatos de pacotes não significa execução
automática. Plasma/SDDM, execução nativa de aplicações Windows, Windows em VM e agentes de IA são metas posteriores,
dependentes das mesmas fundações de processos, rede e segurança.

O primeiro uso será em mídia descartável e QEMU. A ISO só será considerada beta
instalável após passar os critérios de saída e validação no hardware equivalente
ao NAS. Até lá, o sistema é experimental e não deve receber dados reais.

## Princípios e limites

- Kernel e componentes próprios em Rust; alvo x86_64 de 64 bits, boot Limine
  BIOS/UEFI e mensagens próprias em pt_BR.
- Preservar endereços e tamanhos de 64 bits em memória física/virtual, ELF64,
  syscalls, I/O e armazenamento. O allocator cobre até 32 GiB e o boot QEMU já
  valida frames acima de 4 GiB; essa capacidade continua passando por validação
  de regressão junto com os demais gates do kernel.
- Manter o kernel CarlioOS implementado em Rust. Arch Linux é a base de userland desejada, não um
  kernel a ser substituído. Isso exige uma ABI Linux progressivamente
  compatível; não é algo que pacman sozinho resolva.
- Priorizar o papel de servidor/NAS headless, armazenamento e recuperação antes
  de desktop e integrações opcionais.
- Uma função só é “suportada” após testes positivos, negativos e de falha no
  QEMU. Testes em NAS são feitos quando a conexão `nas-wifi` está disponível;
  falha de rota/conexão deixa a validação remota pendente, sem bloquear o
  trabalho local nem ser reportada como sucesso.
- Usar somente imagens e discos descartáveis nos testes de escrita. Não testar
  instalador contra discos de origem ou unidades com dados do NAS.
- Reservar 16 GiB do disco de sistema para logs persistentes e paginação,
  contabilizados como uma área dedicada e configurável no instalador. Perfil
  inicial de projeto: 1 GiB para logs e até 15 GiB para swap; swap só pode ser
  ativado após VM estável e bloco confiável. Não consumir dados de volumes NAS.
- Registrar comando, resultado e log serial em `reports/logs/`, e atualizar o
  relatório da etapa. Nunca guardar segredos em código, logs ou argumentos.

## Estado dos subsistemas do kernel

O critério de conclusão é o comportamento integrado no kernel, com casos
positivos, negativos e de falha; modelos host e detecção PCI não bastam.

| Subsistema | Implementado e validado | Gate que continua aberto |
| --- | --- | --- |
| Processos / scheduler | ELF64 de prova, CR3/RSP0 isolados, preempção, saída/fault/reaper; `destroy(id)` remove Ready/Blocked; APIs internas `clone_thread`/`join_current`; manifesto `ProcessResourceBundle` associado ao `Process`; sobreposição CPL3 pontual em dois APs e reaper com 3 ACKs globais comprovados em BIOS/UEFI local e NAS/KVM BIOS | Ownership direto dos objetos VM ainda fica no registro fixo do backend; stress sustentado concorrente de scheduler, VM e allocator continua sem prova. Também faltam tabela/pool dimensionados conforme recursos, integração `clone`, pilha user própria, futex/POSIX join e estabilidade ABI/VM ([relatório](../reports/fase-2-memoria-execucao.md), [BIOS bundle local](../reports/logs/fase2-process-bundle-qemu-bios-20261119-022753.log), [UEFI bundle local](../reports/logs/fase2-process-bundle-qemu-uefi-20261119-022753.log), [BIOS NAS bundle](../reports/logs/fase2-process-bundle-nas-20261119-022823-qemu-kvm-bios.log), [UEFI NAS bundle](../reports/logs/fase2-process-bundle-nas-20261119-022823-qemu-kvm-uefi.log)) |
| Módulos do kernel (LKMs) | Parser ET_REL host com relocations x86_64, extração de exports da `.symtab`, registro persistente host, lookup kernel/módulos e contabilidade host de dependências; `SmpPageTableManager::protect` com shootdown testado em host ([log fase 2](../reports/fase-2-memoria-execucao.md)) | Ciclo runtime ainda não ligado; backend x86_64 para alocação/mapeamento/invocação, bloqueio de novas chamadas, rollback transacional e quiescência SMP; gates QEMU/NAS que carreguem objetos ET_REL positivos, negativos e de falha |
| Memória | Bitmap até 32 GiB, VM x86-64, folhas user owned, heap expansível e shootdown xAPIC com ACK | Pressão/concorrência geral de VM sob carga; swap permanece desligado |
| Drivers / E/S | PCI read-only, framebuffer Limine, adaptador BlockDevice AHCI e Ethernet Intel QEMU; QEMU validou Hermione em disco descartável | Validação física de AHCI e demais dispositivos (RTL8111/I219, EHCI/xHCI, iGPU), timeout/remoção em hardware |
| VFS | Políticas POSIX host, mounts roteáveis e backends de leitura para Hermione/FAT32/ext4; fixture CPL3 valida `openat/read/close`, leitura serial e negação de arquivo `0600` com `EACCES` em BIOS/UEFI local e NAS/KVM | Descoberta de partições e montagem no boot; syscalls ainda não consultam backends mounted; suporte a arquivos maiores que 16 KiB, nomes longos FAT, recursos ext4 completos, escrita nos mounts e enforcement geral de inode/processo |
| Rede | Intel 82574L QEMU TX/RX polling e DHCPv4 DORA; tabela de sockets bound para bind/listen | TCP/IP de ponta a ponta, `accept/connect/send/recv`, drivers físicos, rota, DNS e serviços |
| Syscalls (SCI) | Entrada x86-64 `SYSCALL`; CPL3 valida identidade/grupos, `getpid`/`gettid`, `clock_gettime`/`clock_getres` monotônicos, `brk` positivo, `mmap/munmap`, VFS inline, serial e rejeições de ponteiros/permissões; BIOS/UEFI QEMU local passam. NAS/KVM BIOS 5 GiB passa; os probes ABI também passam no NAS UEFI | NAS UEFI smoke aberto: a prova de duas CR3 não observou preempção de timer e o probe de page tables falhou ([relatório](../reports/fase-4-aplicacoes-linux.md), [log](../reports/logs/fase4-linux-abi-timers-nas-20261119-021821-qemu-kvm-uefi.log)). Ainda faltam `CLOCK_REALTIME`, `execve` transacional integrado, carregamento do interpretador/dependências ELF dinâmicas, relocations/glibc, GNU version tables, TLS, sinais, futex, chamadas completas de threads, mapeamento de arquivo, mounts no VFS e compatibilidade geral glibc/systemd |
| pegasusDS / servidor de diretório | Contrato ABI v1 congelado como documento; crate local `libcarlio` define interface Rust segura sem transporte de syscall | ABI implementada, capacidades, sandbox, hooks LSM, keyring, AF_UNIX/SCM_RIGHTS, IPv6/TCP/epoll, canais opt-in, verified boot e workload QEMU/NAS; não executar servidor de rede |
| Interrupções | IDT/exceções, PIT/watchdog, timer LAPIC e IPI de TLB com ACK | IRQs de teclado, USB, rede e disco ligados a drivers; tratamento/recovery físico |

A prioridade física é NAS-first para AHCI `8086:1c02`, RTL8111 `10ec:8168`,
EHCI `8086:1c2d/1c26` e vídeo `8086:0152`; depois estação para AHCI
`8086:282a`, I219-LM `8086:15d7`, xHCI `8086:9d2f` e iGPU `8086:1916`.
Veja a [matriz de hardware e prioridade](HARDWARE-BASE.md). Até validação
direta nos dispositivos, estes alvos continuam pendentes.

## Sequência reestruturada

As etapas são gates técnicos: uma etapa pode ser trabalhada em paralelo apenas
quando não depende de recursos ainda ausentes, mas não pode ser marcada como
validada antes de cumprir seus critérios.

### 0 — Contrato de 64 bits, beta e matriz de hardware

Fechar a configuração de produto: CPU x86-64 e execução integral em 64 bits,
BIOS/UEFI, modo headless, discos e
controladores necessários, estratégia de instalação sem apagar dados, política
de atualização/recuperação e conjunto mínimo de serviços do NAS. Manter uma
matriz que separe “detectado”, “driver implementado” e “validado”. Alvos de
referência estão em `docs/HARDWARE-BASE.md`.

**Aceite:** matriz de dispositivos e cenários QEMU definida; critérios de
preservação de discos aprovados; contratos que rejeitam overflow/truncamento em
endereços e tamanhos; boot apenas em long mode; scripts locais reproduzíveis e
logs associados. Este gate é documental e acompanha todas as etapas.

### 1 — Inicialização e hardware mínimo

Estabilizar Limine, memória entregue pelo bootloader, HHDM, exceções, timer,
serial e diagnóstico de falhas. Entregar enumeração PCI somente leitura e uma
prova experimental de bloco AHCI em QEMU. Esta fase não promete drivers do NAS:
AHCI físico, rede Realtek (`10ec:8168`), USB EHCI e vídeo Intel (`8086:0152`)
continuam como trabalho posterior, guiado pela matriz de hardware. A pilha
TCP/IP, DNS, sockets e TLS depende de driver de rede e é posterior.

**Aceite:** boots BIOS/UEFI em QEMU; mapa/HHDM e diagnóstico serial; cenários
positivos e negativos com mídia QEMU descartável; loops de polling limitados e
falhas reportadas sem espera infinita; logs locais reproduzíveis. A validação
no NAS/KVM complementa o gate quando conectável. Timeout/remoção durante I/O e
suporte a controladores físicos ficam registrados como riscos, sem alegação de
suporte físico.

**Estado (2026-11-16): concluída para o escopo QEMU local e NAS/KVM; hardware físico pendente.**
O fechamento final passou `cargo fmt --check`, Clippy com `-D warnings` e 159
testes host localmente e no NAS/KVM; boots BIOS/UEFI passaram em QEMU TCG local
e KVM no NAS. O build de diagnóstico AHCI passou IDENTIFY, escrita/leitura,
erro ATA inválido e ciclo Hermione (formatação descartável, remontagem e scrub)
em disco raw de 16 MiB local e no NAS/KVM. A escrita remota ficou confinada a
`/tmp/carlioos-*`. Não foi induzido timeout real nem remoção física durante
I/O; controladores físicos não foram testados.
Evidências: [relatório da Fase 1](../reports/fase-1-inicializacao.md),
[host/Clippy local](../reports/logs/fase1-fechamento-final-host-20261116-190018.txt),
[QEMU BIOS/UEFI local](../reports/logs/fase1-fechamento-final-qemu-bios-20261116-190018.log),
[NAS/KVM host](../reports/logs/fase1-final2-nas-20261116-185928.txt) e
[AHCI NAS/KVM](../reports/logs/fase1-final2-nas-20261116-185928-filesystem-qemu-kvm.log).

Vídeo: framebuffer RGB de firmware/bootloader consumido pelo kernel, IDs das
iGPUs Intel alvo reconhecidos e pixels escritos por CPU. QEMU BIOS/UEFI local e
NAS/KVM passou. Ainda não há modesetting Intel, aceleração, detecção de
conectores nem controle HDMI/DP. Ver [relatório de vídeo](../reports/fase-video.md).

### 2 — Memória e execução isolada do kernel

Atualização (2026-11-15): a revisão conjunta mais recente passou 106 testes host
localmente e no NAS. O
allocator bitmap agora cobre até 32 GiB; QEMU TCG local BIOS/UEFI com 32 GiB e
QEMU/KVM NAS BIOS/UEFI com 5 GiB alocaram, escreveram, leram e liberaram um
frame acima de 4 GiB. O host NAS tem 8 GiB e não conseguiu reservar 32 GiB para
KVM. QEMU BIOS/UEFI local e NAS/KVM com 4 CPUs comprovam TSS/stack de entrada
por CPU, timer LAPIC, 18 shootdowns remotos e a prova CPL3/reaper no BSP. O
scheduler tem 64 CPUs e 64 slots com filas locais/roubo, mas a carga CPL3
concorrente nos APs e estruturas dinâmicas ainda não foram comprovadas. O probe
agora carrega ELF ET_EXEC através do VFS de boot e monta `argc`/`argv`/`envp`;
esse VFS permanece inline/de teste e não oferece `execve` geral.

Revalidação (2026-11-16): `cargo fmt --check`, Clippy com `-D warnings`, 160
testes host, QEMU TCG BIOS/UEFI com 32 GiB e NAS/KVM BIOS/UEFI com 5 GiB
passaram; os boots verificaram frames acima de 4 GiB e 20 shootdowns com ACK.
`mmap/munmap` CPL3 passou integrado à VMA/ownership/page tables, e o teste
negativo ELF rejeita overflow `u64 → u32`. A fase segue aberta: falta stress
CPL3 concorrente nos APs, ownership completo do bundle de processo e stress
concorrente real de VM/allocator; `brk` não tem probe CPL3 dedicado. Evidências e limites no
[relatório da fase 2](../reports/fase-2-memoria-execucao.md).

Meta de escala para servidor: capacidade de processos configurada pela memória
disponível, filas por CPU, backpressure, contabilização de memória por serviço,
e medições sob carga. Gate inicial de projeto: o teste host já percorre 10 mil
processos leves executáveis em round-robin; faltam contenção e medidas de
memória/latência em hardware e QEMU. Medir esses custos antes de escolher limites. Serviços
de arquivos exigem cache e I/O concorrente com recuperação; hospedagem de VMs
exige backend VT-x/AMD-V e isolamento de dispositivos. Nenhum desses serviços
de servidor está entregue hoje. Consulte o [relatório da fase
2](../reports/fase-2-memoria-execucao.md).

Revalidação do bundle (2026-11-19): host e QEMU BIOS/UEFI local passaram; NAS/KVM
BIOS passou com 5 GiB. NAS UEFI repetiu falha na prova de preempção e no probe
de page tables. O `Process` agora guarda manifesto que encaminha syscall e
reaper ao registro de VM, ainda fixo no backend. Stress sustentado e ownership
direto dos objetos continuam abertos; evidências no relatório da
[Fase 2](../reports/fase-2-memoria-execucao.md).

**Meta de capacidade do kernel:** suportar até 32 GiB de RAM física instalada,
incluindo alocação e mapeamento de frames acima de 4 GiB. O bitmap e os contadores
devem cobrir esse limite sem truncamento; memória acima do limite configurado
deve ser explicitamente ignorada e contabilizada. Aceite inclui boot QEMU com
mais de 4 GiB, alocação/liberação de frames acima da fronteira de 4 GiB e teste
com 32 GiB, além do fallback com menos memória.

O mapa Limine alimenta o allocator de frames; paginação de quatro níveis,
AddressSpace/CR3 independente, ownership das folhas user, heap first-fit global
com expansão por frames, faults CPL3 isolados na prova, filas locais com roubo,
timer LAPIC local e reaper com cleanup genérico fora do lock já existem. A prova
de boot agora cria processos a partir de ELF64 ET_EXEC instalado no VFS,
verifica leitura/execução e monta stack inicial `argc`/`argv`/`envp` no espaço
registrado no scheduler. O VFS ainda é fixo e usa imagens inline de teste; isso
ainda não implementa `execve` geral nem storage persistente. Permanecem
pendentes: stress CPL3 simultâneo nos APs, tabela de processos/pilhas
dinamicamente dimensionadas, ownership integral do bundle de recursos e
expansão do heap de usuário. Tentativas de stress CPL3 nos APs mostraram falha
no retorno ao idle/reaper, então não são consideradas aprovadas. Locks IRQ-safe
protegem scheduler, pool e heap; isso ainda não comprova carga SMP geral de
VM/allocator. O swap continua condicionado à fase de bloco e não é requisito
para habilitar o restante da fase 2.

Incremento SMP de VM (2026-11-15): `SmpPageTableManager` serializa map/unmap
com lock IRQ-safe e exige um `TlbShootdown` explícito cujo contrato aguarda
invalidação remota. O backend xAPIC instala vetor IPI 0xF1, invalida traduções
locais e transmite a todos os APs online. Cada AP recarrega CR3 (e alterna PGE
quando necessário), envia EOI e publica ACK; timeout para o kernel de forma
fail-closed. O caminho arquitetural de `PageTableAccess` agora usa esse backend
no boot. A prova QEMU de quatro CPUs completou 18 shootdowns remotos durante a
prova de VM/runtime. Um teste host faz quatro workers executar 128 ciclos de
map/lookup/unmap e verifica 1024 pedidos pelo contrato. O scheduler de processos
de produção continua BSP-only: APs carregam GDT/IDT e atendem shootdown, além da
tarefa curta de prova; ainda faltam filas/contextos por CPU e timer local para
escalonar processos nos APs. O runtime ainda usa tabela/pilhas de capacidade
fixa e fixtures ELF.
Swap só entra após memória virtual estável e camada de bloco confiável; medir
desempenho e permitir desligá-lo. Swap não substitui RAM e não deve conter
segredos sem proteção apropriada.

`src/sync.rs` agora tem `IrqSpinLock<T>`, que salva/desativa IRQs locais e
restaura o estado após liberar o lock; scheduler, pool de pilhas e heap usam
essa primitiva. Testes host verificam exclusão mútua, mas não validam a
instrução `cli/sti` sob IRQ real. O boot inicia APs e executa uma tarefa
limitada de prova em cada AP. `src/process.rs` mantém scheduler de capacidade
fixa; o BSP roda criação, despacho, troca por timer, CR3/RSP0, fault isolado e
reaping. O backend xAPIC de TLB shootdown foi validado com ACK de todos os APs;
o scheduler de processos segue fora deles e o restante do ciclo VM/allocator
ainda requer locks e coordenação SMP sob carga.

**Aceite para fechar a fase:** alocação/liberação sem sobreposição em toda RAM
até 32 GiB; mapeamentos protegidos; execução ELF originada do VFS; processos
isolados; faults confinados e recursos recolhidos; scheduler e sincronização
SMP validados sob concorrência; syscalls inválidas rejeitadas. **Progresso:**
bitmap estático consome regiões `usable` do Limine até 32 GiB e a prova de boot
acessa frame acima de 4 GiB. `PageTableManager` de quatro níveis edita o
CR3 do Limine, valida map/unmap, invalida o TLB local e aguarda ACK remoto xAPIC
dos APs antes de retornar, além de liberar tabelas vazias; prova BIOS/UEFI
passou em QEMU local e NAS/KVM. A GDT contém seletores ring 3
e a TSS recebe RSP0/IST; a prova controlada CPL3 valida retorno e faults.
`map_user` valida
folhas no half inferior com bit U/S e `AddressSpace` cria PML4 próprio,
compartilha o half superior e mapeia/desmapeia VAs user. O smoke QEMU troca
temporariamente para esse CR3 em CPL0, verifica a VA ausente no CR3 kernel e
retorna com teardown. `OwnedAddressSpace` agora associa frames de folhas aos
processos e recupera frames/tabelas no encerramento explícito, validado em
QEMU BIOS/UEFI local e NAS/KVM. A prova de preempção por timer entre dois CR3s
está integrada ao probe de boot. Faults de processos gerais, scheduler SMP de
produção e loader do VFS seguem pendentes.
`src/heap.rs` contém um
allocator first-fit de região com alinhamento, divisão, coalescência e rejeição
de liberações inválidas; `deallocate` valida a cadeia desde a arena e o payload
exato, sem confiar em metadados adjacentes ao ponteiro recebido. O heap global
protegido usa arena estática de 64 KiB; expansão por páginas segue pendente.
`IrqSpinLock` agora é usado por scheduler, pool de pilhas e heap. SMP detecta
APs, executa uma tarefa limitada em cada AP e atende shootdown xAPIC com ACK;
ainda faltam scheduler nos APs, filas/contextos por CPU e threads persistentes.

Estado atual (2026-11-15): a prova BIOS/UEFI local e NAS executa duas imagens
ELF fixture em espaços owned distintos, com PT_LOAD, permissões RX/NX, stack,
CR3/RSP0, preempção por timer, #PF isolado, SYS_EXIT e reaper. O heap global de
bootstrap usa IrqSpinLock sobre arena estática de 64 KiB. O scheduler mede a ocupação e um
teste exercita 10 mil ciclos de criação/saída/reuso; capacidade continua fixa
por parâmetros const e não valida 10 mil processos simultâneos. Faltam fonte
ELF no VFS, ABI de argumentos/ambiente, allocator expansível seguro fora de
IRQ, scheduler de produção em APs e recursos de processo com ownership
genérico. Evidências no [relatório da fase 2](../reports/fase-2-memoria-execucao.md).
Os leitores FAT32 e ext4 agora implementam o contrato `ReadOnlyMountBackend`;
`Vfs::read_mounted_file` roteia ao backend fornecido pelo chamador. Essa fatia
foi validada em host, mas o boot ainda não instancia volumes nem conecta AHCI,
partições e VFS; portanto, a ISO não faz mounts reais durante a inicialização.

### 3 — Bloco, filesystems e recuperação

**Meta de capacidade:** endereçar discos de até 2 TiB (2^32 setores de 512
bytes) sem truncar LBA, contagem de setores, offsets ou tamanhos em bytes.
Aceite inclui leitura/escrita verificadas no início, perto da fronteira de 2 TiB
e no último LBA em dispositivo/imagem esparsa descartável. O limite de volume
Hermione atual (`u32::MAX` setores) fica aquém de 2 TiB por um setor e precisa
ser corrigido ou formalmente substituído por particionamento/capacidade maior.

Definir `BlockDevice` uniforme, cache/flush, timeout, cancelamento e tratamento
de remoção/erro. Exercitar falhas e cortes de energia em QEMU. Para
interoperabilidade, implementar FAT32 e ext4 primeiro em leitura; adicionar
escrita apenas depois de validar metadados e recuperação. Evoluir Hermione como
formato próprio opcional (não ZFS): árvore de diretórios, alocação, journal ou
protocolo transacional equivalente, snapshots, scrub e reparo somente a partir
de redundância comprovada. SQLite é posterior a filesystem e allocator.

Separar integridade de confidencialidade: CRC/checksum detecta erro acidental;
criptografia exige fonte de entropia, KDF, cifra autenticada, tratamento de
chaves e testes de bloqueio/limpeza. Redundância, scrub persistente e reparo
validado são gates separados. Montagem normal só depois de identificar com
segurança mídia do sistema versus dados.

**Aceite:** montagem somente leitura FAT32/ext4 contra imagens conhecidas;
escrita e remontagem após teste de falha; journal/recuperação em cortes
repetidos; timeout e remoção durante I/O sem corrupção silenciosa; Hermione
recupera apenas cópia validada. Criptografia não será chamada de protegida antes
de testes de senha correta/incorreta, corrupção autenticada e limpeza de chave.

**Estado conhecido:** Hermione v3 em mídia descartável passou testes host e
QEMU, incluindo gravação espelhada, replay do journal após diretório rasgado,
checksum, reparo de uma cópia e rejeição de duas cópias corrompidas. Isso não é
montagem do sistema, cifra, tolerância à falha do disco ou filesystem FAT32/ext4.

**Incremento de mounts (2026-11-16):** VFS delega leitura ao backend tipado de
mount. FAT32 lê caminhos com nomes curtos 8.3; ext4 lê arquivos regulares via
inodes, diretórios lineares e extents no modo somente leitura. Os testes host
passaram e a regressão de boot BIOS/UEFI passou local/NAS. O smoke boot não
monta imagens ext4/FAT32 dentro do kernel. Permanecem fora do escopo: nomes
longos FAT, recursos avançados/journal/checksums ext4, escrita, descoberta de
partições e ligação do driver AHCI ao VFS. Não é suporte completo nem liberação
para testes em discos com dados.

O driver AHCI passa a expor `AhciBlockDevice` para os consumidores do kernel,
com I/O serializado, checagem de LBA e IDENTIFY. O smoke Hermione usa esse mesmo
adaptador e passou em QEMU TCG e NAS/KVM com mídia raw descartável. Ainda não há
parser GPT/partições nem montagem automática; nenhum disco físico foi gravado ou
validado. Os controladores RTL8111, I219-LM, EHCI/xHCI e a integração ext4/FAT32
no boot seguem pendentes.

**Syscalls VFS (2026-11-16):** `open/openat`, `read`, `write`, `lseek` e
`close` agora usam arquivos do VFS inline e descritores limitados por processo;
`fd 0/1/2` continuam ligados à serial. O probe CPL3 abre
`/programas/probe`, lê um byte e fecha o descritor. Host (140 testes), boots
BIOS/UEFI local e NAS/KVM e o smoke AHCI com mídia QEMU descartável passaram.
Isso ainda não conecta descritores aos backends montados Hermione/FAT32/ext4,
não implementa `O_CREAT` e não valida leitura positiva da serial em CPL3.

Hermione/`herm-fs` já tem contrato de bloco, backend AHCI experimental em QEMU,
volume persistente v3, journal redo de metadados, duas cópias de dados e scrub
com reparo quando existe uma cópia válida. O host injeta cortes em cada escrita
e `flush`; em 16/11/2026, QEMU local e NAS/KVM repetiram IDENTIFY, WRITE/READ,
erro ATA, formatar, remontar e scrub em imagens descartáveis. Isso não simula
corte de energia QEMU. Faltam o gate explícito de corte de energia durante
commit, timeout/remoção durante I/O, controladores físicos e montagem normal.
Logs: [QEMU local](../reports/logs/fase2-storage-qemu-local-20261116.log),
[NAS/KVM](../reports/logs/fase2-storage-20261116-nas-20261116-191947-filesystem-qemu-kvm.log).

Atualização de capacidade: o formato e `herm-fs` agora aceitam exatamente
`2^32` setores (2 TiB); teste com imagem esparsa grava/lê o último LBA e rejeita
um setor além. Isso fecha apenas a faixa endereçável no backend de imagem; não
prova AHCI físico nem interoperabilidade.

**Revalidação em imagem descartável (2026-11-16):** `storage-test` local TCG e
NAS/KVM passou IDENTIFY, WRITE/READ, erro ATA, Hermione formatar/remontar/scrub
em raw temporário de 16 MiB. Ver logs [local](../reports/logs/fase2-final-storage-qemu-local-20261116.log),
[NAS/KVM](../reports/logs/fase2-final-20261116-nas-20261116-192713-filesystem-qemu-kvm.log)
e [build](../reports/logs/fase2-final-storage-build-20261116.log). Permanecem abertos
corte de energia QEMU, timeout/remoção durante I/O, montagem normal e
controladores físicos; portanto nenhum journal geral nem confiabilidade para
dados reais é afirmado.

**Requisito de capacidade:** o instalador deverá reservar 16 GiB do disco de
sistema para o espaço do kernel. Planejamento inicial: área de logs persistentes
de 1 GiB e área de paginação de até 15 GiB. Os tamanhos serão configuráveis e
validados contra a capacidade disponível; não haverá ativação de swap até que a
VM, o driver de bloco, flush, falhas e recuperação sejam confiáveis. Ainda não
existe reserva física nem escrita persistente implementada.

### 4 — VFS, contas, permissões e serviços de rede

Construir VFS e mounts sobre filesystems reais; manter organização modular
(`/programas`, `/configuracoes`, `/usuarios`, `/sistema`, `/dados`,
`/temporarios`), mas respeitar os caminhos esperados por programas Linux
(`/usr`, `/etc`, `/var`, `/home`, `/run`, `/dev`, `/proc`, `/sys`). A estrutura
modular é organização e política, não deve quebrar o padrão de caminhos Linux.
Ligar UID/GID, modos, ACL/capabilities conforme necessário, credenciais
protegidas e login a processos isolados. Para o NAS, priorizar rede, SSH/SFTP e
SMB; demais protocolos ficam atrás dos serviços essenciais.

**Aceite:** negação e concessão verificadas no kernel/VFS, isolamento entre
usuários/processos, persistência segura de contas, login real, rede funcional,
autenticação negativa e serviços headless reiniciáveis. VFS/login/catálogo
atuais são modelos host-testados, ainda não conectados a processos ou disco.

**Incremento host (2026-11-15):** o VFS já aplica bits POSIX `rwx` por
owner/group/other, grupos suplementares, `umask`, herança setgid, busca em
componentes de caminho, `chown` restrito a root e execução com bit `x`. O
catálogo usa grupos da sessão para autorizar o programa. São contratos de
biblioteca host e não equivalem à aplicação dessas permissões pelo kernel.
Persistência, operações de arquivo, ACLs e enforcement em processos/syscalls
continuam pendentes. Evidências: [relatório de segurança](../reports/fase-3-seguranca.md).

Atualização: operações host `read_file_as`/`write_file_as` armazenam conteúdo
inline limitado a 16 KiB por arquivo e aplicam permissões POSIX por credencial.
Mounts e armazenamento real seguem pendentes; enforcement no kernel foi
demonstrado somente para concessão/negação de `openat` no fixture descrito a
seguir.

**Prova negativa CPL3 (2026-11-16):** a fixture executa como UID/GID 1000;
`openat` lê arquivo permitido e recebe `EACCES` ao abrir arquivo root-owned
modo `0600`. O smoke exige o marcador em BIOS/UEFI local e NAS/KVM. É evidência
de enforcement no dispatcher para esta operação e fixture, não de login,
persistência, auditoria durável ou autorização geral. Consulte o [relatório e
logs da prova](../reports/fase-3-seguranca.md#correcao--negacao-posix-em-syscall-cpl3--2026-11-16).

**Incremento de namespace (2026-11-16):** VFS cria a hierarquia FHS e mantém
mounts para Hermione, FAT32, ext4, proc e dev, com validação de paths, mount
root-only, opções, seleção específica e delegação de leitura a backends para
Hermione/FAT32/ext4. A delegação ainda é uma API host sem descoberta de partição
ou dispositivo no boot; os leitores FAT32/ext4 são parciais e somente leitura.
ABI Linux `SYSCALL`/dispatcher já está ligada ao contexto de processo para a
fatia implementada. Pacotes Arch seguem em
inspeção `.PKGINFO`, sem extração/transação; a ABI segue insuficiente para
executar pacman ou binários Arch. Evidência: [relatório mounts e namespace](../reports/fase-4-mounts-interoperabilidade.md).

**Incremento de rede (2026-11-16):** driver Intel 82574L/QEMU (`8086:10d3`)
com filas DMA em polling TX/RX e DHCPv4 DORA aprovado no QEMU. O dispatcher
Linux liga `socket`, `bind`, `listen` e `close` a uma tabela bounded IPv4
TCP/UDP por processo, com colisão de portas e restrição de portas privilegiadas.
Ainda faltam drivers RTL8111/I219 físicos, roteamento, DNS, `accept`/`connect`/
`send`/`recv` e teste em hardware. A interface não tem acesso geral à internet.

### 5 — ABI Linux e userlands Arch e Ubuntu

O contrato versionado proposto para integração do pegasusDS está em
[`KERNEL_ABI.md`](KERNEL_ABI.md), com dircache, auditoria e threat model em
documentos próprios. A crate local `libcarlio` ainda não tem transporte de
syscall e a fase permanece aberta; estado e gates em
[`fase-5-pegasusds.md`](../reports/fase-5-pegasusds.md).

Implementar a ABI necessária por fatias: ELF dinâmico x86_64, syscalls e
semântica de processos, sinais, `mmap`, `clone`, `futex`, threads, relógio,
pseudo-filesystems e interfaces de dispositivo. Validar primeiro libc e
ferramentas pequenas; avançar para systemd somente após seus requisitos serem
atendidos. As bases de userland previstas são Arch Linux e Ubuntu sobre a mesma
ABI do kernel CarlioOS. pacman/ALPM e APT/`.deb` terão integração separada.
O console interativo de usuário será zsh quando a ABI suportar seus requisitos;
se essa fatia atrasar, desenvolver um shell Rust com sintaxe/comandos zsh e
compatibilidade de plugins declarada individualmente. O shell serial atual
permanece console de diagnóstico do kernel, sem ser contado como userland.
Adicionar suporte separado para `.deb`, `.rpm`, AppImage, Flatpak (incluindo
Flathub como origem de aplicações), Snap e `.exe`/PE. Para cada formato, medir
separadamente inspeção, instalação, resolução de dependências, execução,
sandbox e atualização/rollback. `.exe` requer uma camada de compatibilidade
Windows ou virtualização; reconhecer o formato não implica executar o programa.
Scripts de instalação privilegiados exigem sandbox/política explícita.

**Aceite:** matriz pública de syscall/programa e formato/operação; ferramentas
base Arch executadas; um pacote representativo de cada formato suportado passa
instalação e verificação, e seus executáveis passam testes de runtime e
isolamento; pacote inválido/repositório não confiável recusados; rollback
comprovado. Compatibilidade será declarada por formato e operação, sem inferir
execução a partir da instalação do pacote. A matriz detalhada de formatos,
operações e pré-requisitos está em [MATRIZ-PACOTES.md](MATRIZ-PACOTES.md).

No primeiro incremento (2026-11-15), o dispatcher tinha contratos para `close`,
`lseek`, `mmap`, `munmap` e `brk`; parser com PIE `ET_DYN` e load bias. O
loader agora aplica `R_X86_64_RELATIVE` em static PIE; dependências, símbolos,
PLT e interpretador dinâmico permanecem sem suporte. Há snapshots de leitura
para `/proc` e `/sys`. Os
VMA por processo agora integra `mmap/munmap/brk` ao ownership das folhas e às
page tables; a prova CPL3 aloca, grava e libera uma página anônima. Mapeamentos
de arquivo/compartilhados e cobertura CPL3 para `brk` seguem pendentes, e os
snapshots ainda não estão montados no VFS. Os modelos host também verificam duas tabelas de página isoladas com a mesma
VA e a persistência do volume Hermione após fechar/remontar a imagem. A entrada
MSR/LSTAR já despacha `SYSCALL` em processos CPL3; `getpid`, `exit`,
`getuid/euid/gid/egid`, `getgroups`, `write` para serial e recusa de destino
inválido em `read` passaram probe BIOS/UEFI local e NAS/KVM. O smoke envia um
byte serial após o aviso do kernel e verifica `read(0)` positivo em CPL3, também
em BIOS/UEFI local e NAS/KVM. O backend por processo faz `mmap/munmap`
anônimo privado com alocação/rollback de frames; `brk` está ligado ao mesmo
estado VMA, mas ainda sem probe CPL3 dedicado. `read` não abre arquivos pelos
backends montados. Leitores de metadados cobrem `.PKGINFO` Arch
e `control` Debian/Ubuntu. As duas rootfs ainda estão pendentes. A ISO do NAS
fica condicionada à conclusão da base Arch. NICs RTL8111/I219-LM estão
identificadas; drivers, TCP/IP, DNS e serviços seguem pendentes.

**Prioridade atual — dispatcher inicial:** manter uma ABI Linux compartilhada
por Arch e Ubuntu. O primeiro gate cobre chamadas de identidade, saída e
console com ponteiros validados e leitura serial positiva/negativa em CPL3;
leitura de arquivos está conectada ao VFS inline. Ainda falta rotear descritores
para backends mounted. `mmap/munmap` anônimo privado já mapeia páginas próprias;
`brk` está conectado ao mesmo VMA/page table, mas falta probe CPL3 dedicado.
Mapeamentos de arquivo/compartilhados, runtime geral, libc/rootfs e linker
continuam pendentes.

**Gate Arch/Plasma:** a syscall `uname` foi adicionada com o layout Linux
x86_64 de seis campos de 65 bytes e validação de ponteiro de saída. Isso é
compatibilidade básica para ferramentas que consultam a identidade do sistema;
não habilita pacman, instalação da base Arch ou Plasma. O loader também aplica
`R_X86_64_RELATIVE` a static PIE, um planejador host ALPM resolve dependências
nomeadas/versionadas em ordem topológica e o snapshot VFS pode ser carregado e
persistido em `rootfs.img` num volume Hermione preexistente; a criação e
remontagem foram verificadas em QEMU com imagem AHCI descartável. O NAS não
ofereceu AHCI utilizável no teste. Isso não é instalação: ainda faltam linker e
relocations dinâmicas, `execve` genérico,
extração/transações ALPM, libc e validação de binários reais; o resolvedor não
cobre toda a semântica libalpm (`vercmp`, `provides`, conflitos).
Permanecem syscalls/infraestrutura de threads, `futex`, sinais, relógio,
dispositivos gráficos/input, compositor e DBus. Plasma/SDDM não está instalável
nem executável nesta etapa.

Histórico: dispatcher host-testável cobre `read` com retorno parcial, EOF e
erros. A camada kernel fornece entrada serial em `fd 0`; rootfs Hermione é
opcional e requer mídia dedicada já formatada. Outros backends de mounts seguem
desconectados das syscalls Linux.

### 6 — Interface, desktop e agentes

Entregar primeiro console/administração headless para o NAS, depois login
gráfico. SDDM, KDE Plasma e componentes visuais inspirados em Budgie/Omarchy
dependem de ABI Linux suficiente, DBus, PAM, compositor, GPU/input e áudio.
Catálogo e loader de apps devem culminar em execução real com permissões. Agente
ChatGPT requer rede/TLS e credencial fornecida pelo usuário; Ollama requer
endpoint local/remoto configurável. Nunca embutir chaves no kernel. Gerenciador
de arquivos e protocolos remotos entram após VFS, permissões e rede.

**Aceite:** login gráfico e sessão recuperável; programas da matriz executados;
operações de arquivos respeitam permissões; conectividade e falhas de
autenticação testadas. Windows Mode em VM licenciada/AD, ARM64 e desktop completo
são expansões posteriores, fora do gate do primeiro beta NAS.

### 7 — Instalador e ISO beta do NAS

Somente após os gates necessários: criar ISO reproduzível, instalador com
prévia, identificação de modelo/capacidade/partições, seleção explícita,
confirmação digitada, proteção contra disco de boot e preservação padrão de
discos de origem. Incluir modo diagnóstico headless, recuperação, atualização e
rollback. Testar em discos virtuais descartáveis e depois hardware equivalente
ao alvo.

**Aceite de saída:** instalação e recuperação repetíveis em BIOS/UEFI; boot
pós-instalação; kernel e userland Arch mínimos; pacman dentro da matriz
suportada; armazenamento e rede requeridos pelo NAS; contas/permissões; logs;
atualização/rollback; teste negativo que prova que discos não selecionados e o
disco de boot não são alterados. Só então publicar como beta instalável para
avaliação. O uso como sistema base do NAS aguarda validação no hardware e
backup restaurável dos dados.

## Itens fora do caminho crítico do primeiro beta

- KDE Plasma/SDDM completos, Windows Mode/AD e agente corporativo em VM.
- ARM64, Bluetooth, Wi-Fi, áudio, FTP e protocolos de desktop não necessários à
  operação headless do NAS.
- SQLite integrado, até filesystem, allocator e isolamento estarem prontos.
- Suporte geral a todos os formatos de pacotes e programas Linux/Windows.

### Plugins e bibliotecas por etapa

- `embedded-graphics`: adaptador opcional `graphics` sobre framebuffer validado;
  não implementa modesetting nem driver Intel.
- `embedded-io`: avaliar ao estabilizar contratos de streams para serial e I/O.
- `acpi`: etapa 1, após mapear tabelas com segurança; nunca tratar endereço
  físico como ponteiro virtual.
- `virtio-drivers`: etapas 1/3, após DMA, BARs, interrupções e ownership.
- `smoltcp`: etapa 4, após NIC e buffers; fornece protocolos, não driver Ethernet.
- `wasmi`: userland nas etapas 4/6, após processos, VFS e limites de recursos;
  plugins não executam em ring 0.

Nesta revisão, somente `embedded-graphics` está integrado ao código. Os demais
permanecem vinculados às etapas e pré-requisitos acima.

### 8 — Compatibilidade nativa com aplicações Windows (pós-beta)

Meta: executar aplicações Windows x86_64 selecionadas diretamente no userland
CarlioOS, sem VM. A compatibilidade ficará em camadas de userland (PE/COFF,
APIs Win32/NT compatíveis e serviços) sobre processos, memória, syscalls, VFS,
rede e gráficos do kernel. Não inclui drivers Windows nem troca do kernel por
NT.

**Aceite:** carregador PE validado, APIs documentadas e matriz de aplicações
com testes positivos/negativos. Ver
[relatório da meta](../reports/fase-8-aplicacoes-windows-nativas.md).

Incremento inicial: `src/userland/windows.rs` valida metadados PE32+ AMD64 com
limites fixos e testes de formatos inválidos. Ele ainda não é carregador nem
permite executar `.exe`; relocação, imports, mapeamento, APIs Win32/NT e
integração com processos permanecem pendentes.

Esses itens continuam objetivos possíveis, mas não devem atrasar armazenamento
seguro, rede, userland Arch mínima e instalador confiável.

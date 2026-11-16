# Relatório — Fase 2: memória e execução

## Índice de CPU no despacho por timer (2026-11-16)

O callback de preempção já recebe o índice da CPU que recebeu a interrupção.
O caminho de ativação do CR3/RSP0 agora usa esse índice diretamente, sem
resolver novamente a CPU via ID LAPIC. Isso mantém a seleção de contexto
coerente com a CPU validada pelo scheduler e evita depender de uma segunda
consulta ao inventário SMP no caminho de troca.

Validação: `cargo test --all-targets` passou (132 testes); `scripts/test-local-iso.sh`
passou QEMU TCG BIOS e UEFI; `scripts/test-nas.sh` passou QEMU/KVM BIOS e UEFI,
com troca de processo/CR3, fault CPL3 isolado, reaper e 18 shootdowns. Logs:
[host](logs/kernel-context-switch-host-20261116-112544.txt),
[BIOS local](logs/kernel-context-switch-qemu-bios-20261116-112544.log),
[UEFI local](logs/kernel-context-switch-qemu-uefi-20261116-112544.log),
[resumo NAS](logs/kernel-context-switch-nas-20261116-112615.txt),
[BIOS NAS](logs/kernel-context-switch-nas-20261116-112615-qemu-kvm-bios.log) e
[UEFI NAS](logs/kernel-context-switch-nas-20261116-112615-qemu-kvm-uefi.log).

Esse ajuste não fecha o gate de stress CPL3 simultâneo nos APs nem elimina os
limites de capacidade estática documentados abaixo.

## `exec` de boot pelo VFS e `argv/env` (2026-11-15)

Modelo host adicional: dois espaços de endereçamento mapeiam a mesma VA user
para frames físicos distintos, preservam CR3s independentes e liberam suas
próprias tabelas. O teste complementa o ownership de páginas; não demonstra
processos concorrentes nos APs.

A criação das tarefas CPL3 de prova agora instala imagens ELF como arquivos no
VFS do kernel, valida `x` e `r`, lê pelo VFS e chama o loader ELF no
`OwnedAddressSpace` que será registrado no scheduler. O boot monta a stack
inicial SysV com `argc`, vetores `argv`/`envp`, terminadores nulos e strings;
testes host cobrem alinhamento, conteúdo/limites e rejeição de NUL interno.
Também foi corrigida a consulta de página do writer ELF para usar a VA alinhada,
necessária para gravar strings em offsets não alinhados.

Esse incremento substitui o carregamento direto da fixture no probe, mas o VFS
de boot ainda é fixo e contém imagens de teste inline: não há syscall geral
`execve`, filesystem persistente montado, arquivos ELF maiores que o limite
inline, nem herança geral de credenciais/descritores. A tabela de processos e o
pool de stacks continuam fixos. O reaper ainda não possui uma barreira validada
que comprove que um AP abandonou a stack antes da liberação; portanto a prova
CPL3 permanece no fluxo seguro já testado e o stress simultâneo nos APs continua
aberto. Tentativas locais de ativar workloads CPL3 longos nos APs produziram
#UD/#GP no retorno/limpeza e não foram aceitas como validação.

Validação confirmada após estabilizar o caminho VFS/ELF: 106 testes host locais
e no NAS; QEMU TCG local e QEMU/KVM no NAS passaram em BIOS/UEFI com dois
espaços de endereço, timer, fault isolado e reaper. Os logs de tentativa SMP com falha são preservados para
diagnóstico e não contam como gate aprovado.
Evidências aprovadas: [host local](logs/exec-argv-env-final-host-20261115-201616.txt),
[BIOS local](logs/exec-argv-env-final-qemu-bios-20261115-201616.log) e
[UEFI local](logs/exec-argv-env-final-qemu-uefi-20261115-201616.log),
[resumo NAS](logs/exec-argv-env-verified-nas-20261115-201901.txt),
[BIOS NAS](logs/exec-argv-env-verified-nas-20261115-201901-qemu-kvm-bios.log) e
[UEFI NAS](logs/exec-argv-env-verified-nas-20261115-201901-qemu-kvm-uefi.log).
Tentativas CPL3 SMP ainda reprovadas: [primeiro stress](logs/exec-argv-env-smp-qemu-bios-20261115-200324.log),
[retorno AP](logs/exec-argv-env-smp-qemu-bios-20261115-201047.log) e
[reaper AP](logs/exec-argv-env-progressive-qemu-bios-20261115-201408.log).

## Incremento VFS→ELF (2026-11-15)

`load_executable_from_vfs` agora valida permissão de execução e leitura, lê o
arquivo com buffer do chamador, valida ELF64 ET_EXEC e carrega segmentos pelo
`ElfLoadTarget`. A API não aloca memória por conta própria e mantém NX/permissões
finais sob responsabilidade do target. Testes cobrem carga autorizada e recusa
por falta de bit de execução antes de qualquer mapeamento. Isso é uma API de
biblioteca: o caminho de `exec` do kernel ainda usa fixture, e `argv`/`env`,
ELF dinâmico e buffers maiores que o VFS inline continuam pendentes.

Validação: 104 testes locais; ISO QEMU TCG BIOS/UEFI e NAS/KVM BIOS/UEFI
passaram. O boot confirma as provas existentes de CPL3, faults isolados,
preempção e reaper, mas não constitui stress concorrente de processos nos APs.
Logs: [host local](logs/fase2-vfs-elf-host-20261115-195049.txt),
[BIOS local](logs/fase2-vfs-elf-qemu-bios-20261115-195049.log),
[UEFI local](logs/fase2-vfs-elf-qemu-uefi-20261115-195049.log),
[resumo NAS](logs/fase2-vfs-elf-nas-20261115-195113.txt),
[BIOS NAS](logs/fase2-vfs-elf-nas-20261115-195113-qemu-kvm-bios.log) e
[UEFI NAS](logs/fase2-vfs-elf-nas-20261115-195113-qemu-kvm-uefi.log).

## Atualização após solicitação de fechamento (2026-11-15)

A suíte completa passou na revisão de SMP e novamente após os incrementos das
fases 3–5: 97 testes da biblioteca, 1 do
`carlio-logs` e 4 do `herm-fs`. QEMU TCG local e QEMU/KVM no NAS passaram em
BIOS e UEFI com 4 CPUs, TSS/stacks por CPU, timer local, 18 shootdowns xAPIC,
prova CPL3 com CR3 independentes, fault isolado, saída e reaper. A expansão do
heap por novas regiões também passou a prova de boot.

Isso não fecha todos os critérios da fase. O scheduler já suporta filas locais
para 64 CPUs e roubo, mas a prova de processos CPL3/reaper ainda roda no BSP;
não há stress real de processos simultâneos nos APs. Tabela/pool são estáticos,
ownership dos recursos não está encapsulado integralmente no `Process`, e o
loader permanece uma fixture: VFS→ELF e ABI `argv`/`env` continuam pendentes.
Não declarar runtime de servidor concluído.

Evidências da revisão final: [testes host locais](logs/fase3-5-fechamento-host-20261115-194459.txt),
[QEMU BIOS](logs/fase3-5-fechamento-qemu-bios-20261115-194459.log),
[QEMU UEFI](logs/fase3-5-fechamento-qemu-uefi-20261115-194459.log),
[testes e execução NAS](logs/fase3-5-fechamento-nas-20261115-194519.txt),
[BIOS NAS/KVM](logs/fase3-5-fechamento-nas-20261115-194519-qemu-kvm-bios.log) e
[UEFI NAS/KVM](logs/fase3-5-fechamento-nas-20261115-194519-qemu-kvm-uefi.log).

## Revisão de fechamento da fase (2026-11-15)

**Validação executada:** 85 testes host; QEMU TCG com 32 GiB em BIOS/UEFI; e
NAS/KVM com 5 GiB em BIOS/UEFI. Todas passaram. O boot com 32 GiB no NAS/KVM
não iniciou porque o host tem 8 GiB e QEMU não conseguiu reservar a memória;
isso não invalida o teste local, mas deixa essa combinação remota indisponível.
Os testes 32 GiB tocaram e verificaram frame acima de 4 GiB pelo HHDM.

A prova de processos agora também provoca #PF real em CPL3 ao acessar uma VA não
mapeada. O kernel registra CR2, encerra só aquela tarefa, volta à outra por
preempção e depois o reaper libera as duas pilhas e os dois espaços de endereço.
Isso valida o caminho de falha do probe; ainda não é política geral de faults
para processos oriundos do VFS.

O lock IRQ-aware passou dois testes host novos e foi integrado ao scheduler,
pool de pilhas e heap. QEMU BIOS/UEFI local com 32 GiB e NAS/KVM com 5 GiB
continuaram passando com o lock presente. Falta teste que force contenção a
partir de uma IRQ real; a integração ainda não cobre toda a VM/allocator SMP.

`IrqSpinLock` foi adicionado: salva IF, desativa IRQs locais, libera o lock antes
de restaurar IF e já protege scheduler, pool de pilhas e heap global. Exclusão
mútua passou testes host; a arquitetura real `cli/sti` ainda carece de teste
dedicado de contenção via interrupção, e a VM/allocator não estão todos sob
locks apropriados para SMP.

**A fase 2 continua aberta.** Gaps de fechamento: scheduler efetivamente nos APs
com filas/contextos por CPU; locks que controlam IRQ e preempção; TLB shootdown
SMP; heap expansível; recursos de processo com ownership genérico; ELF vindo do
VFS com argv/env; e stress de concorrência real no kernel. O dispatcher Linux
começado em `src/linux_abi.rs` é somente host-testado e não substitui esses gates
nem executa userland Arch. A meta de disco de 2 TiB pertence ao gate de bloco da
fase 3.

Logs de fault isolado: [host NAS](logs/fase2-fault-ram32g-nas-20261115-181044.txt),
[BIOS NAS/KVM](logs/fase2-fault-ram32g-nas-20261115-181044-qemu-kvm-bios.log) e
[UEFI NAS/KVM](logs/fase2-fault-ram32g-nas-20261115-181044-qemu-kvm-uefi.log).
Validação mais recente com locks IRQ-aware: [host](logs/fase2-irqlock-32g-nas-20261115-181521.txt),
[BIOS local 32 GiB](logs/fase2-irqlock-32g-qemu-bios-20261115-181500.log),
[UEFI local 32 GiB](logs/fase2-irqlock-32g-qemu-uefi-20261115-181500.log),
[BIOS NAS/KVM 5 GiB](logs/fase2-irqlock-32g-nas-20261115-181521-qemu-kvm-bios.log) e
[UEFI NAS/KVM 5 GiB](logs/fase2-irqlock-32g-nas-20261115-181521-qemu-kvm-uefi.log).

## Meta de capacidade do kernel (2026-11-15)

Meta operacional solicitada: suportar máquinas com até 32 GiB de RAM e discos
de até 2 TiB. O allocator e seu bitmap agora cobrem 32 GiB; testes host cobrem
deduplicação e o limite, enquanto os boots locais BIOS/UEFI com 32 GiB e NAS/KVM
com 5 GiB tocaram frames acima de 4 GiB. O NAS/KVM não comporta convidado de
32 GiB por possuir 8 GiB físicos; essa validação foi feita localmente.

O contrato `BlockDevice` usa LBA e contagem `u64`, e o comando AHCI emite LBA48.
Isso não prova operação em um disco grande. Hermione v3 e `herm-fs` rejeitam
mais de `u32::MAX` setores; como 2 TiB em setores de 512 bytes são `2^32`
setores, falta um setor para atingir a capacidade solicitada. Próximo gate:
aceitar a contagem exata, rever limites de extents/metadados, validar overflow
em offsets em bytes e testar o último LBA com imagem esparsa descartável.
Nenhum teste de disco físico de 2 TiB foi executado.

O requisito de disco ainda não foi implementado: essas metas foram acrescentadas ao [roadmap](../docs/ROADMAP.md), ao
[contrato arquitetural](../docs/ARQUITETURA-KERNEL.md), ao
[projeto Hermione](../docs/HERMIONE.md) e ao [painel de progresso](../progress/index.html).

## ELF em CPL3, heap global e gestão de processos (2026-11-15)

O backend ELF agora é usado pelo runtime de boot: a prova constrói uma imagem
ELF64 ET_EXEC, carrega o segmento PT_LOAD em páginas owned, aplica permissões
RX/NX, cria a stack de usuário e registra cada execução no scheduler com CR3,
frame inicial e pilha kernel próprios. As duas imagens são preemptadas por
IRQ0, encerradas pela chamada ABI SYS_EXIT e recolhidas pelo reaper. A fila
Ready permanece FIFO encadeada O(1). Scheduler::stats expõe capacidade,
ativos, prontos, em execução, bloqueados e encerrados para observabilidade;
um teste exercita 10 mil ciclos de criação, saída, reaping e reutilização de
slot.

O heap first-fit de bootstrap agora fica publicado como serviço global sob
`IrqSpinLock`; inicialização duplicada é recusada e allocate/deallocate passam
pelo lock. A arena continua estática de 64 KiB e ainda não é `GlobalAlloc` do
Rust nem tem expansão por frames. Alocação em IRQ continua proibida por política.

Limite: a imagem ELF ainda é fixture embutida no boot. Não há leitura de
executáveis pelo VFS, ABI de argumentos/ambiente, pilha inicial POSIX, processo
persistente genérico, múltiplas CPUs no scheduler de produção ou tabela sem
limite fixo. A capacidade segue determinada pelos parâmetros const do
Scheduler e pool de pilhas.

Validação final: 81 testes host, boot BIOS/UEFI no QEMU TCG e no NAS QEMU/KVM;
ambos observaram ELF CPL3, saídas, trocas de timer/CR3, heap protegido e zero
erros no shell. Logs em reports/logs/: fase2-runtime-dinamico-* e
fase2-elf-heap-global-final-*.

## Continuação: contratos ELF, ABI e segurança do heap (2026-11-15)

O heap first-fit agora não lê um endereço de cabeçalho guardado junto ao
ponteiro fornecido por `deallocate`. Ele percorre a cadeia de blocos a partir
do início da arena, valida spans e estado, e só aceita o endereço de payload
exato registrado no bloco. Isso rejeita ponteiros interiores/estrangeiros sem
interpretar bytes do payload como endereço de metadados. Alinhamento, divisão,
coalescência e detecção de double-free continuam no allocator de região.

O loader ELF já possui parser e `ElfLoadTarget`; o backend de boot mapeia páginas
owned, copia `PT_LOAD`, mantém BSS zerado, aplica permissões finais NX/W^X e
desfaz páginas em falha. Porém não existe caminho público que combine leitura
de arquivo, criação de espaço, stack/ABI inicial, registro do processo e
despacho pelo scheduler. O teste de preempção ainda usa duas tarefas de prova
e syscall privada; não conta como execução geral de ELF nem ABI de processos
integrada. O heap do boot segue uma arena estática e local ao probe, sem
allocator global protegido ou backing dinâmico por frames.

**Validação do heap:** `cargo fmt --check` e `cargo test --all-targets` passaram
localmente (76 testes da lib, 1 de `carlio-logs` e 3 de `herm-fs`). O primeiro
ensaio detectou divergência na classificação de double-free; corrigida antes
da execução final. QEMU TCG BIOS/UEFI local e `scripts/test-nas.sh` passaram,
incluindo 76+1+3 testes e QEMU/KVM BIOS/UEFI no NAS. Logs finais: [host
local](logs/fase2-loader-heap-host-20261115-173513.txt), [BIOS
local](logs/fase2-loader-heap-qemu-bios-20261115-173513.log), [UEFI
local](logs/fase2-loader-heap-qemu-uefi-20261115-173513.log), [NAS host e
suíte](logs/fase2-loader-heap-nas-20261115-173537.txt), [BIOS
NAS](logs/fase2-loader-heap-nas-20261115-173537-qemu-kvm-bios.log) e [UEFI
NAS](logs/fase2-loader-heap-nas-20261115-173537-qemu-kvm-uefi.log). Esses boots
validam o smoke de runtime existente, não execução geral de ELF nem proteção
de heap global; o próprio kernel informa que o allocator global permanece
desativado.

## Integração do runtime e preempção por timer (2026-11-15)

A prova de boot cria duas tarefas CPL3 com espaços e CR3 distintos, frames
iniciais completos em pilhas kernel exclusivas, e as despacha pela fila do
scheduler. IRQ0 salva o frame atual e seleciona a tarefa seguinte, atualizando
CR3 e RSP0 antes do retorno. Um syscall privado encerra cada tarefa; depois que
o kernel retoma sua própria pilha, o reaper libera as pilhas e destrói os dois
espaços. QEMU BIOS TCG observou trocas de processo e de CR3; a prova `#UD`
continuou isolada e o boot não registrou erros.

Validação daquela revisão: 80 testes host passaram localmente e no NAS; QEMU TCG
BIOS/UEFI passou com 187/169 trocas de timer/CR3, e QEMU/KVM no NAS passou
BIOS/UEFI com 4/4 trocas. O teste exige ao menos uma troca de processo e uma de
CR3, além do reaper e da prova `#UD`. Logs: [host local](logs/fase2-runtime-preempcao-host-20261115-172657.txt),
[BIOS local](logs/fase2-runtime-preempcao-qemu-bios-20261115-172657.log),
[UEFI local](logs/fase2-runtime-preempcao-qemu-uefi-20261115-172657.log),
[host NAS](logs/fase2-runtime-preempcao-nas-20261115-172720.txt),
[BIOS NAS](logs/fase2-runtime-preempcao-nas-20261115-172720-qemu-kvm-bios.log) e
[UEFI NAS](logs/fase2-runtime-preempcao-nas-20261115-172720-qemu-kvm-uefi.log).

Isso é uma prova de boot com duas tarefas, não um scheduler de servidor: tabela
e pool de pilhas do runtime de boot continuam estáticos e limitados a 16
entradas, há uma ABI privada de encerramento, a prova roda no BSP e não carrega
serviços/ELFs arbitrários. Um teste host já exercita 10 mil processos leves em
round-robin, mas o runtime continua fixo e esse resultado não valida contenção
SMP. A meta de servidor exige capacidade configurável pela memória, filas por
CPU, isolamento de serviços e medição de latência/memória sob carga, além de VM
e I/O concorrente.

## Pilhas kernel próprias e fila de prontos encadeada (2026-11-15)

`KernelStackPool` entrega slots exclusivos de pilha alinhados a página; a
criação pelo scheduler transfere o identificador de posse ao `Process`. A
pilha só volta ao pool via `reap`, que exige estado `Exited`, evitando liberar
a memória enquanto o processo ainda está escalonado. A fila Ready passou de
deslocamentos de array a encadeamento FIFO entre slots de processo, com append
e retirada O(1), sem alocação dinâmica em contexto de interrupção. O slot
corrente e cursor de slots livres evitam buscas lineares no despacho/criação.
Ela permanece limitada pela tabela de processos `P`; o heap global ainda não
está ativo. Isso organiza a fila e propriedade, mas não torna a preempção de
processos de produção ativa: o boot ainda não cria processos persistentes nem
faz reaping a partir de uma rotina de usuário/idle.

Validação: 79 testes host, boot BIOS/UEFI no QEMU TCG local e BIOS/UEFI no
QEMU/KVM do NAS. Logs: [host](logs/fase2-fila-processos-dinamica-host-20261115-165701.txt),
[BIOS local](logs/fase2-fila-processos-dinamica-qemu-bios-20261115-165701.log),
[UEFI local](logs/fase2-fila-processos-dinamica-qemu-uefi-20261115-165701.log),
[execução NAS](logs/fase2-fila-processos-dinamica-nas-20261115-165723.txt),
[BIOS NAS](logs/fase2-fila-processos-dinamica-nas-20261115-165723-qemu-kvm-bios.log) e
[UEFI NAS](logs/fase2-fila-processos-dinamica-nas-20261115-165723-qemu-kvm-uefi.log).

**Estado:** em andamento. Allocator físico, tabelas de página, CR3 isolado,
prova CPL3 e validação CPL3 dos contratos de ELF, buffers e scheduler passaram
em QEMU. `OwnedAddressSpace` agora rastreia frames de dados transferidos ou
alocados pelo espaço e os libera junto com as tabelas em teardown explícito.
Ainda faltam faults recuperáveis, carregamento/execução geral ELF e troca de
contexto/preempção.

## Plano sequencial: runtime escalável (2026-11-15)

Ordem de execução registrada para fechar os gaps de runtime:

1. **Fault e teardown genérico:** faults CPL3 passam a produzir
   `ProcessState::Faulted { vector, error_code, address }`. `Scheduler::fault`
   retira a tarefa corrente do despacho; `reap_with` aceita tanto encerramento
   normal quanto fault, roda cleanup fora do lock e preserva os metadados para
   diagnóstico/retry se o cleanup falhar. O IDT encaminha exceções CPL3 de
   processos ativos para esse caminho. Ainda não há bundle de recursos owned
   dentro de `Process`; o runtime de boot mantém os address spaces em fixtures
   estáticas, e fault não recuperável de kernel continua fatal.
2. **Capacidade dinâmica:** pendente. O scheduler e o pool de stacks ainda têm
   limites `const`; depende de armazenamento dinâmico confiável no kernel e de
   rollback transacional de criação.
3. **VFS→ELF e ABI:** pendente. O loader executado é fixture ET_EXEC embutida;
   não monta argv/env POSIX nem amplia o heap de usuário a partir de faults.
4. **SMP de produção:** pendente. Há backend de TLB shootdown com ACK xAPIC,
   mas ainda não há fila/contexto do scheduler de processos por CPU nem proteção
   IRQ-safe cobrindo todo o ciclo de VM/allocator.
5. **Stress concorrente:** pendente. Testes host de fila e heap não substituem
   contenção simultânea no kernel/QEMU entre CPUs.

Esta atualização descreveu uma fatia do passo 1; o backend xAPIC documentado
mais adiante atualiza o estado SMP. A fase permanece aberta.

### Locks de VM compartilhada e contrato de shootdown (2026-11-15)

O allocator físico global do kernel já opera sob `IrqSpinLock`. Foi adicionado
`SmpPageTableManager`, que serializa map/unmap/lookup da mesma árvore com lock
IRQ-safe e chama `TlbShootdown::invalidate_all(root, VA)` depois de alterar as
PTEs. O contrato do trait exige invalidação em todas as CPUs que usam o CR3 e
aguardo de acknowledgements antes do retorno. Um teste host com quatro workers
concorre em 512 VAs e verifica lookup, remoção e os 1024 pedidos de
invalidação.

Limite daquela revisão: ainda não existia backend LAPIC IPI/ACK. O estado foi
superado pela seção “Backend xAPIC e confirmação de TLB remoto” abaixo. Um teste
host adicional aloca e libera 1024 frames por 8 workers sob
`IrqSpinLock`, sem duplicatas. Isso valida contenção do lock host, não
preempção/IRQ reais. Filas/contextos por CPU, VFS→ELF, bundle owned no processo,
estruturas dinâmicas e stress do kernel continuam pendentes.
allocator e QEMU/KVM BIOS/UEFI. Logs: [host final](logs/fase-local-host-20261115-183700.txt),
[BIOS local](logs/fase-local-qemu-bios-20261115-183700.log), [UEFI
local](logs/fase-local-qemu-uefi-20261115-183700.log), [host NAS](logs/fase-1-nas-20261115-183725.txt),
[BIOS NAS](logs/fase-1-nas-20261115-183725-qemu-kvm-bios.log) e [UEFI
NAS](logs/fase-1-nas-20261115-183725-qemu-kvm-uefi.log). Avisos de código
inativo permanecem; não houve falha de teste nem erro serial nos boots.

## Contexto lógico de processo e endurecimento do contrato ELF (2026-11-15)

`Process` agora guarda `CpuContext` com os registradores gerais, RIP, RSP e
RFLAGS. Criação e preempção recusam RIP/RSP fora da faixa user, RFLAGS sem bit
reservado 1 ou com IOPL/NT/VM, e roots de página nulos. `Scheduler::preempt`
salva o contexto validado antes de rotacionar a fila; testes verificam round
robin e restauração do estado completo. O parser ELF agora recusa ET_DYN/PIE
porque ainda não há relocations, além de recusar segmento sem permissões.

Esta é política de scheduler e modelo de contexto, não uma troca de CPU real:
o handler de IRQ ainda retorna ao frame interrompido, o PIT serve ao watchdog,
e faltam stack kernel por processo, frame uniforme com SS/RSP, aplicação de CR3,
controle de preempção dos spinlocks e integração SMP. ELF continua sendo apenas
validador/plano: cópia em páginas próprias, zeragem de BSS, proteção final de
PTEs e entrada em CPL3 ainda não existem. Não anunciar processos executáveis ou
preempção como recurso pronto.

Validação deste incremento: 77 testes host e boots BIOS/UEFI em QEMU TCG e
NAS/KVM; logs: [host](logs/fase2-contexto-final-host-20261115-163720.txt),
[BIOS local](logs/fase2-contexto-final-qemu-bios-20261115-163720.log),
[UEFI local](logs/fase2-contexto-final-qemu-uefi-20261115-163720.log),
[execução NAS](logs/fase2-contexto-final-nas-20261115-163739.txt),
[BIOS NAS](logs/fase2-contexto-final-nas-20261115-163739-qemu-kvm-bios.log) e
[UEFI NAS](logs/fase2-contexto-final-nas-20261115-163739-qemu-kvm-uefi.log).

## Ownership de frames de usuário e ISO de teste físico (2026-11-15)

`OwnedAddressSpace` rastreia cada par VA/PA de dados, rejeita mapeamentos
duplicados, desfaz a alocação quando um mapeamento falha, oferece unmap com
liberação e destruição que devolve o backend para permitir retry em erro. O
backend de boot usa o allocator físico Limine e retorna páginas zeradas. A
prova de CR3 agora usa o espaço com ownership explícito e verifica o teardown
depois de retornar de CPL3 e restaurar o CR3 do kernel.

O teste host cobre duas páginas próprias, permissões, duplicidade, unmap,
liberação de frames e recolhimento das tabelas vazias. `cargo test --all-targets`
passou 74 testes; `scripts/test-local-iso.sh` passou em BIOS/UEFI TCG e
`scripts/test-nas.sh` passou 74/1/3 testes host e BIOS/UEFI KVM. Logs:
[host local](logs/fase-execucao-ownership-host-20261115-163011.txt),
[BIOS local](logs/fase-execucao-ownership-qemu-bios-20261115-163011.log),
[UEFI local](logs/fase-execucao-ownership-qemu-uefi-20261115-163011.log),
[host NAS](logs/fase-execucao-ownership-nas-20261115-163053.txt),
[BIOS NAS](logs/fase-execucao-ownership-nas-20261115-163053-qemu-kvm-bios.log) e
[UEFI NAS](logs/fase-execucao-ownership-nas-20261115-163053-qemu-kvm-uefi.log).

Esses testes não cobrem falha do allocator durante teardown, concorrência real
de page tables, preempção, execução de ELF arbitrário, falta de energia nem
falha física de RAM. A ISO resultante é somente para boot/avaliação manual;
não há instalador nem persistência segura.

## Validação CPL3 dos novos contratos (2026-11-15)

O stub user emite a chamada privada `SYS_KERNEL_ROUTINE_PROBE` por `int 0x80`.
Ela só é tratada enquanto a prova controlada está ativa; não faz parte da ABI
de produção. No handler, já em CPL0 e usando o CR3 user ativo, a suíte valida
ELF64 válido e W^X inválido, copia ELF do espaço user e cobre leitura/escrita
cross-page, página somente leitura, VA não mapeada e overflow. Também exercita
criar dois processos, despachar em duas CPUs lógicas, rotacionar, bloquear,
acordar, encerrar e rejeitar CPU inválida. Todos os 11 bits de resultado são
exigidos pelo smoke BIOS/UEFI. O código do programa roda CPL3; as funções de
kernel são executadas no handler CPL0, conforme o desenho normal de syscalls.

Isso valida as rotinas e a fronteira de teste, não um loader ou scheduler de
produção: ELF ainda não é transformado em mapas executáveis, os processos de
prova não são persistentes e não ocorre troca de registradores/CR3 por timer.
Também não há aplicação geral do dispatcher de syscalls ao processo atual.

Validação final: 69 testes da biblioteca, 1 de `carlio-logs` e 3 de `herm-fs`;
boots BIOS/UEFI em QEMU TCG local e QEMU/KVM NAS. Logs: [host](logs/fase-2-ring3-rotinas-host-20261115-160139.txt),
[BIOS local](logs/fase-2-ring3-rotinas-qemu-bios-20261115-160139.log),
[UEFI local](logs/fase-2-ring3-rotinas-qemu-uefi-20261115-160139.log),
[execução NAS](logs/fase-2-ring3-rotinas-nas-20261115-160355.txt),
[BIOS NAS](logs/fase-2-ring3-rotinas-nas-20261115-160355-qemu-kvm-bios.log) e
[UEFI NAS](logs/fase-2-ring3-rotinas-nas-20261115-160355-qemu-kvm-uefi.log).

## Contratos iniciais de ELF, buffers e processos (2026-11-15)

`src/elf.rs` valida ELF64 little-endian x86_64 sem alocação: limites dos
cabeçalhos/tabela, segmentos `PT_LOAD`, overflow, faixa user, segmentos
sobrepostos, W^X e entry point executável. O retorno é um plano fixo de
segmentos; não copia a imagem, aplica relocations/PIE, cria páginas nem inicia
um programa.

`src/user_memory.rs` verifica overflow, faixa inferior user e todas as páginas
tocadas, incluindo bit U/S e escrita quando exigida. `copy_from_user` e
`copy_to_user` validam a faixa completa antes da cópia; a prova as chama com a
tabela de páginas ativa. O dispatcher normal ainda não possui syscalls gerais
de buffers, e page faults/ownership ainda faltam.

`src/process.rs` adiciona tabela fixa, estados Ready/Running/Blocked/Exited,
acordar/bloquear/encerrar e fila global protegida por spinlock. O lock impede
que duas CPUs despachem o mesmo PID simultaneamente. Trata-se do escalonador de
metadados: não salva contexto, troca CR3, preempta pelo timer, libera processos
encerrados ou está ligado aos APs do boot. O scheduler SMP real e processos
gerais seguem pendentes até integrar troca de contexto e ownership do VM.

Os logs desse incremento anterior estão arquivados; a validação CPL3 atualizada
e a evidência de vídeo estão referenciadas nas seções acima e no
[relatório de vídeo](fase-video.md).

## ABI inicial de syscalls e prova CPL3 (2026-11-15)

`src/syscall.rs` define ABI x86_64 com número em RAX, argumentos em RDI, RSI,
RDX, R10, R8 e R9 e retorno em RAX. As chamadas sem ponteiros implementadas
são `SYS_ABI_VERSION`, `SYS_GETPID` e `SYS_UPTIME_TICKS`; números desconhecidos
retornam `-ENOSYS`. O gate DPL3 `int 0x80` usa esse dispatcher e o código de
prova em CPL3 consulta seu PID. Testes host cobrem as três respostas e número
inválido. Isso ainda não é uma ABI de processo de produção: PID/ticks vêm de
contexto de prova, não há syscall de buffers, ELF, scheduler ou terminação de
processos; falhas continuam isoladas apenas durante a prova controlada.

Validação final do incremento: 55 testes da biblioteca, 1 do `carlio-logs` e
3 do `herm-fs`; ISO e smoke BIOS/UEFI no QEMU TCG local e BIOS/UEFI no QEMU/KVM
do NAS. Em todos os boots o marcador exige `SYS_GETPID` via `int 0x80` em CPL3.
Evidências: [host local](logs/fase-2-syscall-final-host-20261115-153500.txt),
[BIOS local](logs/fase-2-syscall-final-qemu-bios-20261115-153500.log),
[UEFI local](logs/fase-2-syscall-final-qemu-uefi-20261115-153500.log),
[execução NAS](logs/fase-2-syscall-final-nas-20261115-154020.txt),
[BIOS NAS](logs/fase-2-syscall-final-nas-20261115-154020-qemu-kvm-bios.log) e
[UEFI NAS](logs/fase-2-syscall-final-nas-20261115-154020-qemu-kvm-uefi.log).

## Entrega deste incremento

- Bitmap estático de disponibilidade e bitmap separado de ownership, sem uso de
  heap na inicialização.
- Só páginas completas em regiões `usable` podem ser alocadas. Regiões
  reservadas permanecem indisponíveis; memória abaixo de 1 MiB é excluída.
- A janela física coberta vai de 1 MiB até 4 GiB. O limite é explícito e
  endereços fora da capacidade são ignorados.
- `allocate`/`free` validam alinhamento, pertencimento a uma região disponível e
  propriedade da página; duplicatas no mapa não contam frames duas vezes.
- O boot aloca uma página, escreve e lê por HHDM, zera a palavra de teste e
  devolve a página. O QEMU smoke exige a mensagem de sucesso.

## Validação

- Local: 31 testes da biblioteca e 2 do utilitário (`herm-fs`), build da ISO,
  boots BIOS/UEFI em QEMU TCG. O teste de boot reportou 130.576 frames livres
  em BIOS e 118.171 em UEFI, com alocação/acesso/liberação aprovados.
- NAS/KVM cabeado: mesmos 33 testes host e boots BIOS/UEFI; allocator reportou
  130.576 frames em BIOS e 119.195 em UEFI, com a prova de acesso aprovada.
- Logs host/local: [`host`](logs/fase-2-memoria-host-20261115-133713.txt),
  [`BIOS`](logs/fase-2-memoria-qemu-bios-20261115-133713.log) e
  [`UEFI`](logs/fase-2-memoria-qemu-uefi-20261115-133713.log).
- Logs NAS: [`execução`](logs/fase-2-memoria-nas-20261115-133737.txt),
  [`BIOS`](logs/fase-2-memoria-nas-20261115-133737-qemu-kvm-bios.log) e
  [`UEFI`](logs/fase-2-memoria-nas-20261115-133737-qemu-kvm-uefi.log).

## Limites e próximos passos

## Tabelas de página e mapeamento

- `PageTableManager` percorre a árvore x86_64 de quatro níveis e opera sobre o
  CR3 atual. O backend separa acesso físico/ownership, alocação de tabelas
  zeradas e invalidação TLB, sem exigir heap.
- `map` valida canonicalidade do endereço virtual, alinhamento/alcance físico e
  flags permitidas para páginas supervisor. Recusa colisões e entradas huge;
  rollback libera tabelas criadas se a folha não puder ser instalada.
- `unmap` retorna o frame da folha, invalida o TLB local e libera tabelas
  intermediárias vazias. O frame de dados continua sob responsabilidade de
  quem chamou.
- Prova no boot usa o CR3 carregado pelo Limine: mapeia uma VA temporária para
  frame do allocator, escreve/lê o conteúdo pela VA, desmapeia e recupera o
  frame. O QEMU smoke exige a mensagem de sucesso.
- Host: 35 testes passaram, incluindo validações de endereço/flags, conflito
  huge, colisão, duplo unmap, invalidação e recuperação das tabelas vazias.
  BIOS e UEFI passaram em QEMU TCG local e QEMU/KVM no NAS cabeado.
- Logs desta etapa: [`host local`](logs/fase-2-paginacao-host-20261115-134638.txt),
  [`BIOS local`](logs/fase-2-paginacao-qemu-bios-20261115-134638.log),
  [`UEFI local`](logs/fase-2-paginacao-qemu-uefi-20261115-134638.log),
  [`host NAS`](logs/fase-2-paginacao-nas-20261115-134704.txt),
  [`BIOS NAS`](logs/fase-2-paginacao-nas-20261115-134704-qemu-kvm-bios.log) e
  [`UEFI NAS`](logs/fase-2-paginacao-nas-20261115-134704-qemu-kvm-uefi.log).

O allocator físico e este backend continuam single-core, sem locks SMP. A
árvore editada ainda é a do Limine; não se troca CR3 por espaços independentes,
não há proteção user/kernel completa nem gestão de page faults. O protótipo de
heap descrito abaixo ainda não é o allocator global do kernel.

## Mapeamento de páginas de usuário (2026-11-15)

`map_user` aceita somente endereços canônicos no half inferior, seta U/S na
folha e nas entradas intermediárias e rejeita flags inválidas. `mapping`
retorna endereço físico e flags efetivas. `map` mantém a política supervisor e
recusa explicitamente a flag U/S. O backend exige que o chamador forneça um
frame exclusivo do processo: ele ainda não valida ownership e a tabela continua
sendo o CR3 global do Limine.

A prova de boot aloca um frame físico, mapeia e acessa uma VA supervisor, depois
uma VA user no lower half; valida a flag U/S, desmapeia ambas e devolve o frame.
O acesso é feito por código CPL0, então ainda não comprova execução ring 3 nem
isolamento. A invalidação TLB continua local à CPU; não há shootdown SMP.

Host: 55 testes passaram, incluindo map_user, flags supervisor/user, VA fora do
half de usuário e conflitos. A mesma prova passou em QEMU BIOS/UEFI TCG local e
NAS/KVM. Logs: [host](logs/fase-2-mapeamento-usuario-host-20261115-150825.txt),
[BIOS local](logs/fase-2-mapeamento-usuario-qemu-bios-20261115-150825.log),
[UEFI local](logs/fase-2-mapeamento-usuario-qemu-uefi-20261115-150825.log),
[execução NAS](logs/fase-2-mapeamento-usuario-nas-nas-20261115-151208.txt),
[BIOS NAS](logs/fase-2-mapeamento-usuario-nas-nas-20261115-151208-qemu-kvm-bios.log)
e [UEFI NAS](logs/fase-2-mapeamento-usuario-nas-nas-20261115-151208-qemu-kvm-uefi.log).

## CR3 independente por processo — primeiro protótipo

`AddressSpace<A, MAPPINGS>` aloca um PML4 próprio, copia as entradas de kernel
do half superior, restringe seus mapas ao half user, acompanha VAs para
desmapeamento e libera as tabelas do processo sem liberar frames de dados. O
boot cria esse CR3, mapeia o frame de prova, verifica que o CR3 do kernel não
contém a VA, troca temporariamente o CR3, acessa a página a partir de CPL0,
retorna ao CR3 original e executa teardown. Isso valida troca de CR3 e
compartilhamento do mapeamento do kernel; ainda não executa `iretq` para ring 3
nem contém falhas de usuário.

Host: 56 testes passaram. BIOS/UEFI passou em QEMU TCG local e NAS/KVM. Logs:
[BIOS local](logs/fase-2-espaco-virtual-qemu-bios-20261115-151712.log),
[UEFI local](logs/fase-2-espaco-virtual-qemu-uefi-20261115-151712.log),
[host NAS](logs/fase-2-espaco-virtual-nas-20261115-151747.txt),
[BIOS NAS](logs/fase-2-espaco-virtual-nas-20261115-151747-qemu-kvm-bios.log) e
[UEFI NAS](logs/fase-2-espaco-virtual-nas-20261115-151747-qemu-kvm-uefi.log).

Limites: allocator e page tables ainda não são protegidos por locks SMP;
invalidação de TLB é local; a API depende do chamador para fornecer frames user
exclusivos; faltam page faults recuperáveis, troca de contexto, ring 3 e
ownership/refcount de páginas compartilhadas.

## Heap de região (protótipo)

`src/heap.rs` implementa allocator first-fit sobre memória fornecida pelo
chamador, com cabeçalhos alinhados, suporte a layouts/alinhamentos, divisão de
blocos, coalescência ao liberar, checagem de ponteiro estrangeiro e detecção de
liberação repetida/corrupção. A API exige acesso mutável exclusivo; não possui
locks e não faz mapeamento de páginas. A região deve estar previamente mapeada
e reservada. Assim, o kernel ainda não usa `alloc` nem declara heap ativo.

Três testes cobrem alinhamento grande e escrita, reutilização após coalescência,
ponteiros inválidos, exaustão e região curta. A suíte passou com 47 testes
host. O boot agora fornece uma região estática alinhada de 64 KiB, exercita
alocação alinhada, escrita/leitura e liberação em BIOS e UEFI e exige essa
evidência no smoke QEMU. Essa região é apenas uma prova de boot: o kernel
continua sem allocator global e sem `alloc`. Evidências: [testes
host local](logs/fase2-heap-prototipo-host-20261115-142249.txt), [BIOS
local](logs/fase2-heap-prototipo-qemu-bios-20261115-142249.log), [UEFI
local](logs/fase2-heap-prototipo-qemu-uefi-20261115-142249.log), [host
NAS](logs/fase2-heap-prototipo-nas-20261115-142449.txt), [BIOS
NAS](logs/fase2-heap-prototipo-nas-20261115-142449-qemu-kvm-bios.log) e [UEFI
NAS](logs/fase2-heap-prototipo-nas-20261115-142449-qemu-kvm-uefi.log).

`src/sync.rs` agora fornece `SpinLock<T>` baseado em `AtomicBool`/`UnsafeCell`,
com guard RAII e testes host de contenção e oito threads concorrentes. É
fundação de sincronização, ainda não usada pelo kernel; não desabilita IRQ nem
preempção e não é seguro adquiri-la de handlers de interrupção.

Próximos passos: ownership de frames por processo, CR3/VM independentes,
rollback seguro de page tables, page faults isolados e shootdown TLB SMP;
integrar locks IRQ/preempção-safe e heap global com testes concorrentes no
kernel. Depois validar `iretq`/retorno ring 3, ABI de syscalls, loader ELF e
scheduler multicore. Swap continua após VM estável e bloco confiável.

## Validação incremental: heap de boot e sincronização (2026-11-15)

O QEMU smoke agora exige a mensagem da prova do heap. Após adicionar a prova de
região estática e `SpinLock<T>`, a suíte passou com 49 testes (45 biblioteca,
1 `carlio-logs`, 3 `herm-fs`); BIOS e UEFI passaram em TCG local e KVM no NAS.
O teste concorrente host usa oito threads e 32 mil incrementos protegidos. Isso
valida o lock no host, não SMP no kernel. Logs: [host](logs/fase2-spinlock-host-20261115-143213.txt),
[BIOS local](logs/fase2-spinlock-qemu-bios-20261115-143213.log),
[UEFI local](logs/fase2-spinlock-qemu-uefi-20261115-143213.log),
[host NAS/KVM](logs/fase2-spinlock-nas-20261115-143231.txt),
[BIOS NAS/KVM](logs/fase2-spinlock-nas-20261115-143231-qemu-kvm-bios.log) e
[UEFI NAS/KVM](logs/fase2-spinlock-nas-20261115-143231-qemu-kvm-uefi.log).

## SMP inicial via Limine

O kernel pede a resposta MP do Limine, reserva uma stack de 16 KiB para cada
slot de CPU (até 64), publica o trampoline para os APs com store atômico
Release e espera sua confirmação com Acquire. O trampoline desabilita
interrupções, troca para a stack reservada e grava processor ID, APIC ID e
estado online em slots atômicos exclusivos. Mailboxes por AP recebem uma tarefa
limitada de prova e retornam resultado via atomics Release/Acquire. O AP volta
ao polling depois; não há troca de contexto, fila geral nem tarefas persistentes.
O BSP não compartilha allocator, shell, IDT/TSS ou drivers com eles.

O smoke QEMU usa quatro vCPUs e exige quatro detectadas/três APs iniciadas.
Também foi validado o fallback com uma vCPU, sem APs, que continua o boot. A
versão final passou em BIOS/UEFI local TCG e NAS KVM; logs:
[host local](logs/fase2-smp-final-host-20261115-143852.txt),
[BIOS local](logs/fase2-smp-final-qemu-bios-20261115-143852.log),
[UEFI local](logs/fase2-smp-final-qemu-uefi-20261115-143852.log),
[fallback single-core](logs/fase2-smp-singlecpu-qemu-20261115-143905.log),
[host NAS](logs/fase2-smp-final-nas-20261115-143922.txt),
[BIOS NAS](logs/fase2-smp-final-nas-20261115-143922-qemu-kvm-bios.log) e
[UEFI NAS](logs/fase2-smp-final-nas-20261115-143922-qemu-kvm-uefi.log).

O despacho limitado de tarefas foi validado com quatro vCPUs, três APs e três
resultados, em BIOS/UEFI local e NAS/KVM. O fallback de uma vCPU também passou.
Evidências: [BIOS local](logs/fase2-smp-dispatch-qemu-bios-20261115-144355.log),
[UEFI local](logs/fase2-smp-dispatch-qemu-uefi-20261115-144355.log),
[single-core](logs/fase2-smp-singlecpu-qemu-dispatch.log),
[host NAS](logs/fase2-smp-dispatch-nas-20261115-144423.txt),
[BIOS NAS](logs/fase2-smp-dispatch-nas-20261115-144423-qemu-kvm-bios.log) e
[UEFI NAS](logs/fase2-smp-dispatch-nas-20261115-144423-qemu-kvm-uefi.log).

## Logs e capacidade de sistema

O kernel oferece `logs`, que imprime em Markdown o buffer limitado de erros em
RAM; ele é volátil e se perde no reboot. A ferramenta nativa Rust `carlio-logs`
agrega capturas `.log`/`.txt` em tabelas e gráficos textuais e pagina arquivos
Markdown no terminal. O painel agregado está em
[`painel-validacao.md`](logs/painel-validacao.md). Local e NAS/KVM passaram o
gerador e os boots BIOS/UEFI após a integração; o QEMU smoke também verifica o
comando `logs` pela serial.

Requisito do produto registrado: reservar 16 GiB em disco para logs persistentes
e paginação, com perfil inicial de 1 GiB para logs e até 15 GiB para swap. Essa
reserva ainda não foi alocada: persistência aguarda filesystem/bloco confiável,
e swap aguarda VM estável, conforme o roadmap.

## Preparação inicial para ring 3

A GDT do kernel agora inclui descritores de código/dados ring 3, e a TSS recebe
uma pilha `RSP0` dedicada para entrada por interrupções e mantém IST para
double fault. O boot valida o seletor TR e DPL dos descritores. BIOS/UEFI local
e NAS/KVM passaram; logs: [`host`](logs/fase-2-ring3-inicial-host-20261115-135801.txt),
[`BIOS`](logs/fase-2-ring3-inicial-qemu-bios-20261115-135801.log),
[`UEFI`](logs/fase-2-ring3-inicial-qemu-uefi-20261115-135801.log) e
[`NAS/KVM`](logs/fase-2-ring3-inicial-nas-20261115-135822.txt).

Isso prepara segmentos e stack, mas não faz `iretq` para CPL3. Faltam espaço de
endereços independente, mapeamento de usuário, troca/validação de contexto, ABI
de syscalls e execução ELF. Heap kernel, ring 3 executável, ELF, syscalls e
scheduler seguem gates explícitos; swap depende de VM e bloco confiáveis.

## Revalidação integrada (2026-11-15)

Após GDT/TSS, ferramenta Markdown de logs, CLI de imagem Hermione e leitor FAT32
host, passaram 44 testes (40 biblioteca, 1 `carlio-logs`, 3 `herm-fs`).
BIOS/UEFI passou local e NAS/KVM; a build `storage-test` passou AHCI/Hermione v3
em mídia descartável local e NAS. Evidências e pendências da Fase 3 estão no
[`relatório de bloco/filesystems`](fase-3-bloco-filesystems.md).
## Mapeamento de segmentos ELF e recursos de contexto (2026-11-15)

`ElfImage::load_into` agora percorre segmentos `PT_LOAD`, cria páginas user
zeradas, copia o payload, mantém BSS zerado e aplica permissões finais somente
depois da cópia. O backend x86 usa frames próprios, marca páginas sem execução
por padrão e aplica NX/RW conforme os flags ELF; segmentos que compartilham uma
página são recusados. Falhas tentam desfazer todos os mapeamentos da operação.

O registro de processo agora reserva campos para o topo da pilha de kernel e o
frame de interrupção salvo. A IDT tem primitiva para ativar um CR3 e atualizar
TSS.RSP0, exigindo IRQ local suspensa e mapeamento compartilhado do kernel. O
stub de IRQ agora aceita frame de retorno substituído; IRQ0 originada em CPL3
chama o escalonador, salva o endereço do frame na pilha atual, seleciona o
próximo e troca CR3/RSP0 antes de `iretq`. Há também inicializador de frame user
para primeira execução. A API de criação executável recusa recursos nulos ou
alinhamentos inválidos.

Na revisão registrada abaixo, a preempção de duas tarefas ELF e o reaper de
pilhas/espaços passaram a ser exercitados em uma prova de boot. O escopo ainda é
limitado à fixture: o loader não está conectado ao VFS, à ABI `argv`/`env` nem a
um contrato genérico de recursos por processo.

Validação desta etapa: `cargo test --all-targets` (78 testes), boot QEMU TCG
BIOS/UEFI e QEMU KVM no NAS BIOS/UEFI passaram. Logs: `fase2-timer-preempcao-final`
em `reports/logs/`. O LFS 12.0 foi adotado como referência para toolchain e
compatibilidade de userland; limites documentados em
[`docs/REFERENCIA-LFS-12.md`](../docs/REFERENCIA-LFS-12.md).

## Fault CPL3, reaper genérico e heap expansível (2026-11-15)

A prova de runtime agora provoca um #PF real em CPL3, registra CR2, encerra a
tarefa faulting e deixa a outra tarefa prosseguir. Ao fim da prova, o reaper
libera as pilhas do pool e destrói os dois espaços owned, incluindo frames de
dados e tabelas. Esta execução continua específica da prova:
O scheduler agora oferece `reap_with`: transiciona `Exited` para `Reaping`,
invoca cleanup fornecido pelo dono fora do lock e só remove o processo/devolve
a pilha após sucesso. Falha no callback restaura `Exited` para retry. A prova
CPL3 usa essa API para destruir o espaço owned e liberar a pilha no mesmo ciclo.
Isto generaliza o protocolo de coleta, mas o armazenamento dos espaços ainda é
fixture global; faltam um bundle de recursos owned no runtime e faults
recuperáveis/pagináveis.

O heap global agora usa um conjunto de arenas first-fit. Quando uma alocação
falha nas arenas existentes, o caminho reserva um frame físico fora do lock do
heap, usa o HHDM e anexa uma região de 4 KiB; a liberação localiza a arena
proprietária. O boot forçou crescimento sob pressão, escreveu/leu dados e
liberou as alocações. Permanecem o limite de 256 regiões, arenas novas de 4 KiB,
alocações maiores que 4 KiB dependentes da arena inicial e ausência de stress
concorrente prolongado.

Validação do reaper: `cargo test` (93 testes), incluindo callback fora do lock,
reuso da pilha/slot e restauração após falha; `git diff --check` passou. Boot
QEMU TCG 32 GiB BIOS/UEFI local e NAS/KVM 5 GiB BIOS/UEFI passaram com fault e
cleanup integrados. Logs locais: [host](logs/fase-local-host-20261115-182954.txt),
[BIOS](logs/fase-local-qemu-bios-20261115-182954.log),
[UEFI](logs/fase-local-qemu-uefi-20261115-182954.log). Logs NAS:
[host e resumo](logs/fase-1-nas-20261115-183019.txt),
[BIOS](logs/fase-1-nas-20261115-183019-qemu-kvm-bios.log) e
[UEFI](logs/fase-1-nas-20261115-183019-qemu-kvm-uefi.log).
Validação anterior do heap/fault: `cargo test` (91 testes), QEMU TCG 32 GiB BIOS/UEFI local;
NAS/KVM 5 GiB BIOS/UEFI também passou. `cargo check --target
x86_64-unknown-none` não é um comando válido para todos os alvos do workspace,
pois inclui ferramentas host com `std`; o script de imagem compila o binário do
kernel corretamente. Logs locais: [host](logs/fase-local-host-20261115-182234.txt),
[BIOS](logs/fase-local-qemu-bios-20261115-182234.log),
[UEFI](logs/fase-local-qemu-uefi-20261115-182234.log). Logs NAS:
[host e resumo](logs/fase-1-nas-20261115-182258.txt),
[BIOS](logs/fase-1-nas-20261115-182258-qemu-kvm-bios.log) e
[UEFI](logs/fase-1-nas-20261115-182258-qemu-kvm-uefi.log).

Próximos gates da fase: mover o bundle de recursos para ownership por processo
e generalizar a entrada de faults; ligar VFS, loader ELF e ABI `argv`/`env`; implementar filas e contextos
reais por CPU com o backend de shootdown já disponível; expandir a tabela/pilhas
de forma dinâmica e executar stress concorrente antes de declarar o runtime escalável.

### Backend xAPIC e confirmação de TLB remoto (2026-11-15)

Foi adicionado o backend xAPIC em `src/arch/x86_64/lapic.rs`. O BSP mapeia a
página física do LAPIC em uma VA kernel como PCD/PWT antes de acessar MMIO,
habilita o vetor IPI fixo 0xF1 e dispara para todos os APs online exceto o BSP.
Cada AP instala os seletores de código/dados do kernel e a IDT compartilhada,
habilita seu LAPIC e só então é publicado como online. O handler recarrega CR3;
quando PGE está ativo, desliga e restaura PGE para invalidar também traduções
globais; depois envia EOI e publica o ACK com ordenação Release. O BSP aguarda
todos os ACKs com Acquire antes de continuar. Timeout é fail-closed e mantém a
CPU parada, pois não é seguro reutilizar frames após um ACK ausente.

O `BootPageTableAccess::invalidate_page` agora usa o backend e aguarda ACKs
remotos antes de retornar. O smoke de QEMU em 4 CPUs passou BIOS, validando
18 shootdowns no ciclo VM/runtime; o log inclui `TLB SMP: shootdowns enviados=18`.
O smoke exige essa linha. O teste host permanece útil para serialização do
contrato `SmpPageTableManager`, mas não substitui a validação xAPIC real.

**Limite importante:** isso conclui o caminho de APs para IPI e invalidação TLB,
mas não conclui o escalonamento SMP de processos. `Scheduler<16, 1>`, timer de
processos e troca de CR3/RSP0 seguem no BSP; APs ainda executam apenas a tarefa
de prova e o handler de IPI. Permanecem necessários timer local por CPU, filas
e contextos por CPU, TSS/pilhas de entrada independentes e testes concorrentes
de alocação/VM/processos antes de chamar o runtime de multiprocessador.

Validação deste backend: `cargo fmt --check`, `git diff --check` e 96 testes
host passaram localmente e no NAS. A ISO passou QEMU TCG BIOS/UEFI local e
QEMU/KVM BIOS/UEFI no NAS, todos com 4 CPUs e 18 shootdowns confirmados. Logs
da revisão final: [host local](logs/smp-lapic-final-host-20261115-185716.txt),
[BIOS local](logs/smp-lapic-final-qemu-bios-20261115-185716.log),
[UEFI local](logs/smp-lapic-final-qemu-uefi-20261115-185716.log),
[host/NAS e resumo](logs/smp-lapic-final-nas-20261115-185758.txt),
[BIOS NAS](logs/smp-lapic-final-nas-20261115-185758-qemu-kvm-bios.log) e
[UEFI NAS](logs/smp-lapic-final-nas-20261115-185758-qemu-kvm-uefi.log).

### Base de escalonamento SMP e entrada por CPU (2026-11-15)

O kernel agora reserva GDT/TSS e stack de entrada ring 0 independentes para
cada CPU (até 64), instala o timer periódico local xAPIC nos APs e direciona
as interrupções para o scheduler com o índice da CPU que recebeu o timer. O
BSP mantém o PIT para preempção e watchdog. O scheduler do kernel foi ampliado
para 64 processos e 64 CPUs: filas prontas locais distribuem inserções e CPUs
ociosas podem roubar trabalho; a propriedade `Running { cpu }` segue exclusiva.
Saída e faults usam a CPU atual e a ativação de CR3/RSP0 atualiza o TSS local.

O timer e o despacho dos APs só são liberados após as provas de boot e o reaper,
para que interrupções frequentes não concorram com shootdowns durante a limpeza
de tabelas. O teste CPL3 e o reaper de boot continuam executados pelo BSP. A
capacidade é ainda estática e limitada a 64 processos/CPUs; o caminho não inclui
alocação dinâmica de tabela, nem um teste de carga que execute processos CPL3
simultaneamente em vários APs. Esses são gates restantes antes de declarar o
uso SMP de processos completo.

Validação desta etapa: 97 testes host; QEMU TCG BIOS/UEFI e QEMU/KVM BIOS/UEFI
no NAS, todos com 4 CPUs, 18 shootdowns e reaper do teste confirmados. Logs:
[host local](logs/fase-smp-processos-ciclo-host-20261115-193003.txt),
[BIOS local](logs/fase-smp-processos-ciclo-qemu-bios-20261115-193003.log),
[UEFI local](logs/fase-smp-processos-ciclo-qemu-uefi-20261115-193003.log),
[resumo NAS](logs/fase-smp-processos-ciclo-nas-20261115-193105.txt),
[BIOS NAS](logs/fase-smp-processos-ciclo-nas-20261115-193105-qemu-kvm-bios.log) e
[UEFI NAS](logs/fase-smp-processos-ciclo-nas-20261115-193105-qemu-kvm-uefi.log).

## Mapa VMA inicial para `mmap`, `munmap` e `brk` (2026-11-16)

Foi adicionada em `src/vma.rs` uma tabela ordenada de capacidade fixa para
reservas de usuário: `mmap` alinhado com first-fit e reutilização de buracos,
`munmap` com divisão de regiões, política W^X e controle de colisão/crescimento
de heap por `brk`. Quatro testes host cobrem alinhamento, reutilização, split,
overflow, proteção inválida e colisão entre heap e mapeamento. A tabela ainda é
metadado: não aloca frames nem publica mapeamentos nas page tables; syscalls do
runtime ainda não possuem esse backend. Portanto `mmap/brk` permanecem
inoperantes via syscall até integrar VMA, ownership físico e address space do
processo com rollback em falhas.

Validação local completa da ISO em BIOS/UEFI: [host](logs/vma-foundation-20261116-host-20261116-122153.txt),
[BIOS](logs/vma-foundation-20261116-qemu-bios-20261116-122153.log),
[UEFI](logs/vma-foundation-20261116-qemu-uefi-20261116-122153.log). A validação
NAS executou 139 testes da biblioteca e os smokes BIOS/UEFI em KVM: [resumo](logs/kernel-context-switch-nas-20261116-122218.txt),
[BIOS](logs/kernel-context-switch-nas-20261116-122218-qemu-kvm-bios.log) e
[UEFI](logs/kernel-context-switch-nas-20261116-122218-qemu-kvm-uefi.log).

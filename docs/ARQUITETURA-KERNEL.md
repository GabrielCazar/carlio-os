# Arquitetura do kernel CarlioOS

## Requisito arquitetural: 64 bits

O alvo do sistema é x86-64 (x86_64) de ponta a ponta: long mode, kernel Rust
64-bit, paginação x86-64, processos em ring 3, imagens ELF64 e ABI de 64 bits.
Endereços, tamanhos, contagens e offsets devem permanecer em tipos com largura
suficiente (`u64`/`usize`) e cada conversão estreita precisa validar faixa e
overflow. Drivers e formatos externos podem ter campos menores por especificação,
mas a conversão deve ser explícita e validada.

A meta de capacidade é 32 GiB de RAM física. O allocator bitmap já cobre frames
até esse limite e aloca/toca/libera frame acima de 4 GiB. QEMU TCG local passou
com 32 GiB em BIOS/UEFI; NAS/KVM passou com 5 GiB. O host NAS tem 8 GiB, então
QEMU/KVM não conseguiu reservar 32 GiB nele. O layout virtual
deve respeitar os limites canônicos do processador e manter separação entre
espaço user e kernel. CPUs que não oferecem long mode não são alvo.

A camada de bloco deve endereçar discos de até 2 TiB, usando LBAs e contagens
de setores de 64 bits ponta a ponta. O transporte AHCI monta campos LBA48, mas
isso sozinho não comprova suporte do kernel: capacidade do dispositivo, limites
de DMA, particionamento e filesystem também precisam cobrir o último setor sem
overflow. Hermione v3 aceita exatamente `2^32` setores (2 TiB) no backend de
imagem; a faixa foi testada no último LBA com imagem esparsa. Isso ainda não
comprova controlador AHCI físico nem montagem do filesystem no boot.

## Camadas e contratos

1. **Boot e plataforma:** protocolo Limine, CPU, mapa de memória, HHDM,
   exceções e inicialização serial. Valida respostas do bootloader antes de
   expor recursos às camadas seguintes.
2. **HAL x86_64:** operações de porta PCI, MMIO, DMA, interrupções, relógio e
   primitivas de sincronização. Código dependente da arquitetura fica aqui.
3. **Barramentos e drivers:** descoberta PCI/USB e drivers AHCI, VGA,
   xHCI/EHCI, Bluetooth, Wi‑Fi e Ethernet. Drivers recebem recursos validados
   e retornam `Result`; não alteram diretamente serviços de alto nível.
4. **Serviços do kernel:** blocos, rede, entrada, vídeo, áudio e armazenamento.
   Convertem erros de hardware em erros tipados e mantêm política fora do
   driver.
5. **Sistema e interface:** Hermione, utilitários e terminal. Não acessam
   registradores ou DMA diretamente.

## Resiliência e segurança

- Toda espera de hardware tem prazo finito; timeouts geram erro tipado e log.
- Falhas de um dispositivo desabilitam aquele dispositivo e deixam os demais
  disponíveis. Inicialização não essencial pode continuar em modo degradado.
- O kernel não usa heap antes do allocator; estruturas iniciais têm tamanho
  fixo e limites verificados.
- MMIO e DMA só são usados depois de validar endereço, tamanho, alinhamento e
  faixa de memória. Nenhum driver escreve em discos reais durante testes.
- O logger de boot usa buffer fixo e saída serial best-effort; falha de serial
  não pode bloquear o kernel.
- Interfaces de driver não são uma fronteira de segurança enquanto tudo roda
  em ring 0. Isolamento de drivers/processos requer memória protegida, syscalls
  validadas e execução menos privilegiada, planejados para a fase 2.
- O linker hostável de LKMs tem parser ELF64 ET_REL, extrai exports da
  `.symtab`, guarda-os em registro persistente host e acompanha dependências
  retornadas pelo resolver; unload exige quiescência no contrato do backend.
  `SmpPageTableManager::protect` altera flags supervisor e aguarda shootdown
  remoto, com teste host de preservação do frame/NX. Ainda não existe backend
  runtime x86-64 de alocação/mapeamento/invocação ligado ao allocator e às page
  tables; LKMs não são carregáveis pelo kernel. Um módulo futuro roda em ring 0 e pertence
  ao domínio de confiança do kernel. O backend exige ABI versionada,
  relocations ELF64, W^X, bloqueio de novas chamadas, referências ativas,
  quiescência SMP e rollback.

## Estado

Esta arquitetura é o contrato alvo. O allocator físico bitmap importa regiões
`usable` do mapa Limine até 32 GiB. O `PageTableManager` edita a árvore de
quatro níveis e valida map/unmap. `AddressSpace` cria um PML4, compartilha
entradas do half kernel e mantém folhas user rastreadas. `ProcessVm` integra
VMA, folhas owned e page tables para `mmap/munmap` anônimo privado, com rollback;
o probe CPL3 aloca, escreve e libera uma página. O scheduler guarda os objetos `ProcessVm`
em slots tipados, com lock por slot. O snapshot copiável de `Process` carrega
o identificador do slot e o CR3. O kernel valida PID e raiz antes de acessar,
substituir ou liberar o objeto. As threads acessam a VM do líder do grupo.
Exceções CPL3 em tarefas despachadas registram vetor, código e endereço de
fault. O reaper seleciona tarefas terminais da tabela e libera recursos
depois que a barreira confirma a saída das CPUs dos contextos de usuário.

O scheduler usa filas locais e roubo entre CPUs. O salvamento do frame, o
reenfileiramento e a escolha da próxima tarefa ocorrem sob o mesmo lock.
Limine inicia os APs em pilhas separadas, com RSP0, IST e timer LAPIC por CPU.
A validação de quatro CPUs executou 8.192 ciclos de mmap/munmap e alocação
no heap em cada um de dois APs, seguidos de dois page faults isolados. O
reaper verificou o retorno à contagem inicial de frames.

O heap first-fit é o `GlobalAlloc` do kernel. Quando nenhuma arena atende
ao layout, o allocator físico reserva frames contíguos e o kernel os anexa
ao heap via HHDM. Novas arenas podem ocupar várias páginas e permanecem
disponíveis para reúso após a liberação dos blocos.

IRQs salvam o frame na pilha interrompida e usam uma pilha de 64 KiB por CPU,
com quatro fatias de 16 KiB. `SYSCALL` e `int 0x80` das tarefas escalonadas
executam na pilha da tarefa. O diagnóstico `stack-watermark` registra a marca
acumulada observada após cada tipo de entrada. Nos APs de stress, a varredura
ocorre ao fim da carga para reduzir interferência. A marca inclui a execução
da instrumentação, e o build padrão mantém a medição desabilitada.

Cada tarefa tem uma pilha de kernel de 128 KiB. O caminho de `execve` de disco
excedeu a reserva anterior de 64 KiB durante a validação. A pool ainda usa armazenamento estático
contíguo e, portanto, não tem guard pages. O próximo desenho deve reservar uma
faixa virtual exclusiva por pilha, mapear frames físicos somente na região de
uso e deixar ao menos uma página inferior sem mapeamento. A pilha de IRQ por CPU
já é separada da pilha da tarefa e deve continuar independente. A migração
depende de um alocador de frames de kernel com rollback, uma faixa virtual que
não colida com os mapeamentos Limine/HHDM, teardown após quiescência de todas as
CPUs que possam usar a pilha e diagnóstico de #PF que identifique guard-page
faults. Até esses contratos estarem ligados, as pilhas atuais permanecem sem
proteção por guarda virtual.

O descritor `Process` carrega um manifesto `ProcessResourceBundle` com token
para VM no backend, raiz CR3 e stack user. Syscalls e reaper consultam o
manifesto; teardown usa esse token. Os objetos `ProcessVm` continuam em
registro fixo do backend. Ownership completo, contabilidade, rollback e stress
aguardam build e gates; não declarar a expansão concluída até passarem.
Evidências e limites no [relatório da fase 2](../reports/fase-2-memoria-execucao.md).

Para cargas grandes de VM e arquivos, o desenho alvo precisa provisionar
processos e memória conforme recursos disponíveis, aplicar limites e pressão de
memória, manter filas por CPU e I/O assíncrono, e medir latência e throughput.
Servir VMs também requer backend VT-x/AMD-V e isolamento de dispositivos; isso
não está implementado. Ver metas e evidências no [relatório da fase
2](../reports/fase-2-memoria-execucao.md) e no [roadmap](ROADMAP.md).

O kernel ainda roda principalmente em ring 0 e tem shell monolítico, com uma prova restrita de CPL3. Há contratos host-testáveis de parsing ELF64, validação de buffers e fila/processo multicore; não estão ligados a execução geral. Já possui timeouts na serial, um log de erros fixo,
captura serial de panic, IDT para exceções 0–31 com registradores/CR2,
watchdog PIT e IST para double fault. IRQs externas além de IRQ0 continuam
desativadas. O driver AHCI experimental identifica e lê/escreve no QEMU usando
DMA e imagem descartável. Ainda faltam dumps duráveis no kernel, matriz de
controladores físicos, interface de bloco genérica, demais drivers e isolamento.
A implementação será migrada em etapas; cada recurso só muda para “suportado”
após testes positivos, negativos e de falha no QEMU local e no NAS.

O loader ELF64 percorre o caminho de criação e execução do probe CPL3: segmento
PT_LOAD, permissões finais, stack, CR3/RSP0, preempção e SYS_EXIT. A prova instala
a fixture ELF pelo VFS e monta `argc`/`argv`/`envp`. O syscall `execve`
limitado prepara uma VM nova, registra intervalos ELF/stack nas VMAs e troca
CR3 após atualizar a imagem do processo; ainda não foi validado em QEMU/NAS.
A tabela do scheduler e o pool de pilhas continuam com capacidade fixa. Um #PF
user é isolado na prova e leva ao encerramento/reaping daquela tarefa. O heap
first-fit valida a cadeia da arena e o payload exato; o serviço global usa
`IrqSpinLock` e expande arenas por frames, com stress concorrente prolongado
ainda pendente.

O preparador ELF agora tem caminho hostável para `PT_INTERP`: carrega programa
e interpretador ET_DYN sem aplicar relocations de userland, inicia pelo entry
do interpretador e prepara os campos básicos de `auxv` (`AT_PHDR`, `AT_PHENT`,
`AT_PHNUM`, `AT_PAGESZ`, `AT_BASE`, `AT_ENTRY`). O `ld-linux` resolveria
dependências e relocations. Esse preparador está ligado ao `execve` limitado,
mas ainda não oferece dependências/relocations completas de userland. A
capacidade de arquivos inline VFS (16 KiB), os buffers fixos de montagem e a
ausência de persistência ainda impedem a userland Arch real.
Testes host sintéticos não validam execução de glibc ou userland Arch.

O comando `logs` formata o buffer volátil em Markdown na serial. Persistência
dos logs integra a reserva planejada de 16 GiB no disco de sistema (meta inicial
de 1 GiB para logs e até 15 GiB para swap), ainda sem escrita em disco; swap só
será habilitado após VM estável e bloco confiável. A ferramenta host `carlio-logs`
resume capturas seriais em Markdown e tem visualizador paginado em Rust.

A GDT define descritores kernel/user e a TSS tem RSP0 para entrada por
interrupção e IST para double fault. A prova controlada valida transição
`iretq`, syscalls iniciais e faults em CPL3. Execução geral de ELF, runtime
persistente, isolamento de serviços e sincronização SMP seguem pendentes.

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
o probe CPL3 aloca, escreve e libera uma página. Ownership dos frames mapeados
por processo já existe, e a prova confina um #PF user, encerra a tarefa e
recolhe seus recursos. `SmpPageTableManager` serializa map/unmap e o backend
xAPIC aguarda ACKs remotos; stress concorrente de VM/allocator e política geral
de faults continuam pendentes.
O projeto exige kernel multithread em SMP. Limine agora inicia APs em stacks
separadas e cada uma grava seu estado inicial atômico. Cada AP processa tarefas
limitadas via mailbox; após a prova inicial, timer LAPIC e despacho SMP podem
escalonar processos por CPU. Há filas locais e roubo em tabela fixa de 64
processos/CPUs, `CpuContext` validado, pool com ownership de pilhas privilegiadas
e `IrqSpinLock` no scheduler, pool e heap. Os logs anteriores só mostravam
preempção CPL3 em dois APs, não uma testemunha de sobreposição temporal. O
código agora marca entrada/saída CPL3 por CPU e exige CR3 kernel confirmado por
BSP e APs antes do reaper. A revalidação BIOS/UEFI local confirmou que
`read(0)` recebe e copia o byte serial; as probes BIOS/UEFI locais e NAS/KVM
agora confirmam sobreposição CPL3 em dois APs e ACK global do reaper em três
APs, após validar CR3 kernel e stack de idle. Isso é uma prova pontual, não
stress sustentado de scheduler/VM/allocator. O bundle completo de recursos ainda não
pertence ao `Process`; não é runtime persistente nem base dimensionada
dinamicamente.

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
a fixture ELF pelo VFS e monta `argc`/`argv`/`envp`, mas não há `execve` geral.
A tabela do scheduler e o pool de pilhas continuam com capacidade fixa. Um #PF
user é isolado na prova e leva ao encerramento/reaping daquela tarefa. O heap
first-fit valida a cadeia da arena e o payload exato; o serviço global usa
`IrqSpinLock` e expande arenas por frames, com stress concorrente prolongado
ainda pendente.

O preparador ELF agora tem caminho hostável para `PT_INTERP`: carrega programa
e interpretador ET_DYN sem aplicar relocations de userland, inicia pelo entry
do interpretador e prepara os campos básicos de `auxv` (`AT_PHDR`, `AT_PHENT`,
`AT_PHNUM`, `AT_PAGESZ`, `AT_BASE`, `AT_ENTRY`). O `ld-linux` resolveria
dependências e relocations. Esse preparador ainda não está ligado a uma syscall
Linux `execve` nem à substituição transacional de processo; a capacidade atual
de arquivo inline VFS (16 KiB) também não comporta o interpretador Arch real.
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

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
overflow. Hermione atualmente limita o volume a `u32::MAX` setores e ainda não
alcança exatamente 2 TiB.

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

## Estado

Esta arquitetura é o contrato alvo. O allocator físico bitmap importa regiões
`usable` do mapa Limine até 32 GiB. O `PageTableManager` edita a árvore de
quatro níveis e valida map/unmap. `AddressSpace` cria um PML4, compartilha
entradas do half kernel e mantém folhas user rastreadas; o smoke troca para o
CR3 próprio em CPL0 e retorna com teardown. Ownership dos frames mapeados por
processo já existe, e a prova de boot confina um #PF user, encerra a tarefa e
recolhe seus recursos. Política genérica de faults, shootdown TLB e sincronização
SMP ainda não estão resolvidos.
O projeto exige kernel multithread em SMP. Limine agora inicia APs em stacks
separadas e cada uma grava seu estado inicial atômico. Cada AP pode processar
uma tarefa limitada de prova via mailbox e depois volta ao polling. Há uma fila
global de processos em memória fixa, `CpuContext` validado e pool com ownership
de pilhas privilegiadas. A prova de boot cria duas tarefas CPL3 com CR3s
distintos; IRQ0 salva e restaura frames e atualiza CR3/RSP0, e o reaper libera
pilhas e espaços encerrados. A prova é temporária, de uma CPU e de capacidade
fixa (16 processos/pilhas); não é ainda runtime para serviços persistentes nem
base dimensionada para servidor. Spinlocks não controlam preempção e o scheduler
não está integrado aos APs.

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

O loader ELF64 agora percorre o caminho de criação e execução do probe CPL3:
segmento PT_LOAD, permissões finais, stack, CR3/RSP0, preempção e SYS_EXIT.
A origem continua uma fixture embutida, sem integração ao VFS nem ABI para
argumentos/ambiente. A tabela do scheduler e o pool de pilhas continuam com
capacidade fixa configurada em compilação. Um #PF user é isolado na prova e
leva ao encerramento/reaping daquela tarefa. O heap first-fit valida a cadeia
da arena e o payload exato; seu serviço global de bootstrap usa `IrqSpinLock`,
mas ainda tem arena estática de 64 KiB e não possui expansão por frames.

O comando `logs` formata o buffer volátil em Markdown na serial. Persistência
dos logs integra a reserva planejada de 16 GiB no disco de sistema (meta inicial
de 1 GiB para logs e até 15 GiB para swap), ainda sem escrita em disco; swap só
será habilitado após VM estável e bloco confiável. A ferramenta host `carlio-logs`
resume capturas seriais em Markdown e tem visualizador paginado em Rust.

A GDT define descritores kernel/user e a TSS tem RSP0 para entrada por
interrupção e IST para double fault. A prova controlada valida transição
`iretq`, syscalls iniciais e faults em CPL3. Execução geral de ELF, runtime
persistente, isolamento de serviços e sincronização SMP seguem pendentes.

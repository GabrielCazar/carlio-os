# Arquitetura do kernel CarlioOS

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
  validadas e execução menos privilegiada, planejados para a fase 3.

## Estado

Esta arquitetura é o contrato alvo. O kernel ainda roda inteiramente em ring 0
e tem shell monolítico. Já possui timeouts na serial, um log de erros fixo,
captura serial de panic, IDT para exceções 0–31 com registradores/CR2,
watchdog PIT e IST para double fault. IRQs externas além de IRQ0 continuam
desativadas. O driver AHCI experimental identifica e lê/escreve no QEMU usando
DMA e imagem descartável. Ainda faltam dumps duráveis no kernel, matriz de
controladores físicos, interface de bloco genérica, demais drivers e isolamento.
A implementação será migrada em etapas; cada recurso só muda para “suportado”
após testes positivos, negativos e de falha no QEMU local e no NAS.

# Hardware de referência do kernel

Inventário somente leitura coletado nesta máquina e em `nas-wifi` em
15/11/2026. Os IDs PCI orientam detecção e testes. Não acessar nem alterar
discos reais durante o desenvolvimento; testes de bloco usam imagens
descartáveis no QEMU.

| Classe | Máquina de desenvolvimento | Servidor NAS |
| --- | --- | --- |
| Armazenamento | SATA Intel RST/AHCI `8086:282a`; SSD SATA de 256 GB; leitor SD Realtek `10ec:525a`; cartão SD MMC | SATA AHCI Intel `8086:1c02`; 3 discos SATA observados: Seagate `ST9160314AS` (149,1 GiB), Toshiba `MQ01ABD100` (931,5 GiB), Toshiba `MQ01ACF050` (465,8 GiB) |
| Rede cabeada | Intel I219-LM `8086:15d7` | Realtek RTL8111 `10ec:8168` |
| Wi-Fi | Intel Wireless 8265 `8086:24fd` | Adaptador USB Realtek `0bda:f179` (“802.11n” no sysfs); conectado à árvore USB EHCI `8086:1c2d`; interface Linux observada |
| USB | Intel xHCI `8086:9d2f` | Intel EHCI `8086:1c2d` e `8086:1c26`; sysfs enumera receiver Dell `413c:301b`, teclado USB `c0f4:05f5` e Wi-Fi Realtek `0bda:f179` |
| Vídeo | Intel HD Graphics 520 `8086:1916` | Intel integrada `8086:0152` |
| Áudio | Intel HD Audio `8086:9d70` | Intel HD Audio `8086:1c20` |

O NAS não tem `lsusb` instalado; o adaptador Wi-Fi USB foi identificado por
sysfs. A coleta também confirmou CPU Intel Core i5-3470, 8 GiB RAM, chipset
H61, BIOS 4.6.5 (11/10/2022), iGPU `8086:0152`, RTL8111 `10ec:8168`, AHCI
`8086:1c02`, dois controladores EHCI, HDA `8086:1c20` e SMBus `8086:1c22`.
O Linux lista três discos SATA (modelos acima). Esses fatos descrevem a máquina;
não significam que CarlioOS tenha drivers para esses dispositivos. A rede SSH
usada para a coleta comprova conectividade do ambiente atual, não do kernel.

## Validação da matriz no NAS — 15/11/2026

Inventário remoto somente leitura confirmou os IDs PCI e modelos de CPU,
memória, placa, BIOS, discos e Wi-Fi USB registrados acima. `lsusb` está ausente,
mas o adaptador foi identificado em `/sys/bus/usb/devices/1-1.4` como Realtek
`0bda:f179`. `lspci` mostrou EHCI `8086:1c2d`/`8086:1c26`, AHCI `8086:1c02`,
áudio `8086:1c20`, vídeo `8086:0152` e rede `10ec:8168`.

**Resultado:** matriz de inventário **atualizada e confirmada contra o Linux do
NAS**. Compatibilidade física CarlioOS **não validada**: não foi feito boot
direto nem I/O em discos reais. Os testes anteriores em QEMU/KVM usam
dispositivos virtuais e não substituem validação de driver físico. Evidência
sanitizada: [coleta](logs/inventario-hardware-nas-20261115.txt).

## Drivers necessários

| Driver | Hardware base | Estado | Etapas principais |
| --- | --- | --- | --- |
| VGA compatível | VGA legado em QEMU | Kernel escreve pixels no framebuffer RGB configurado pelo Limine; buffer de células VGA compatível host-testado. Não programa registradores VGA nem seleciona modo de vídeo | console/renderização, detecção de modo, fallback serial |
| Intel iGPU | `8086:1916` local; `8086:0152` NAS | IDs reconhecidos; se o firmware entrega framebuffer Limine, kernel escreve nele por CPU. Sem driver Intel nativo ou aceleração | inicialização iGPU, memória gráfica, modos e aceleração |
| HDMI / DisplayPort | Conectores dependentes do controlador gráfico | O kernel usa o framebuffer configurado pelo firmware quando encaminhado pelo Limine; não enumera conectores nem treina links HDMI/DP | driver de saída específico por GPU, EDID, hotplug e modos |
| SATA AHCI | `8086:282a` local; `8086:1c02` NAS; QEMU `8086:2922` | Caminho de IDENTIFY somente leitura habilitado para os três IDs; DMA/RW/journal testados apenas em QEMU `8086:2922` | Fazer boot no hardware físico com mídia preservada; validar portas, timeout, remoção e recuperação |
| USB | xHCI `8086:9d2f` local; EHCI `8086:1c2d/1c26` NAS | Detecção PCI parcial; driver não implementado | enumeração, teclado/armazenamento, reset e timeout |
| Bluetooth | Adaptador USB local ainda não identificado; NAS sem controlador Bluetooth identificado (receiver Dell `413c:301b` não classificado como Bluetooth) | Não implementado | HCI sobre USB, firmware e descoberta de controlador |
| Ethernet | Intel I219-LM `8086:15d7`; Realtek RTL8111 `10ec:8168`; Intel 82574L QEMU `8086:10d3` | Driver polling Intel 82574L/QEMU com filas DMA e DHCPv4 DORA validado em QEMU; I219/RTL8111 físicos pendentes | Validar TX/RX/link em hardware; drivers RTL8111/I219, interrupções, roteamento, DNS e TCP |
| Wi-Fi | Intel Wireless 8265 `8086:24fd` PCI local; Realtek `0bda:f179` USB no NAS | Hardware NAS identificado no sysfs; driver Wi-Fi do CarlioOS não implementado | enumeração USB, firmware, associação, autenticação e gestão de energia |
| Áudio HDA | `8086:9d70` local; `8086:1c20` NAS | Não implementado | PCI, codec, buffers DMA e saída básica |

Ordem de implementação: boot/mapa de memória/HHDM; PCI e AHCI; framebuffer RGB;
xHCI/EHCI; Ethernet; Bluetooth; Wi-Fi; HDA. O kernel deve continuar headless
quando vídeo, rede ou áudio falharem.

## Prioridade de compatibilidade física do kernel

O alvo prioritário é o conjunto real desta estação de desenvolvimento e do
NAS, nesta ordem. A compatibilidade só muda para “validada” após teste positivo,
negativo e de falha no dispositivo físico; QEMU/KVM é evidência complementar e
não substitui esse gate. Testes de armazenamento físico devem ser somente leitura
até existir autorização e mídia descartável confirmada.

| Prioridade | Equipamento/dispositivo | Identificadores | Trabalho do kernel |
| --- | --- | --- | --- |
| P0 | Boot/headless nos dois equipamentos | x86_64, Limine BIOS/UEFI | Boot, serial, mapa/HHDM, IDT, watchdog, diagnóstico e fallback sem GPU |
| P1 | Controlador SATA da estação e do NAS | Intel AHCI `8086:282a`, `8086:1c02` | Driver AHCI, DMA seguro, IDENTIFY/read-only, limites de timeout e erros; validar discos reais sem escrita |
| P1 | Ethernet do NAS e da estação | Realtek RTL8111 `10ec:8168`; Intel I219-LM `8086:15d7` | Driver PCI/DMA e link; depois TX/RX, DHCP, TCP/IP e serviços headless |
| P2 | USB para entrada e Wi-Fi do NAS | xHCI `8086:9d2f`; EHCI `8086:1c2d`, `8086:1c26`; Wi-Fi USB Realtek `0bda:f179` | Host controller, enumeração, teclado/armazenamento USB e depois firmware/associação Wi-Fi |
| P2 | Vídeo local e NAS | Intel HD 520 `8086:1916`; Intel integrada `8086:0152` | Preservar framebuffer Limine; depois modesetting, EDID e conectores |
| P3 | Wi-Fi da estação e áudio | Intel 8265 `8086:24fd`; HDA `8086:9d70`, `8086:1c20` | Firmware/autenticação Wi-Fi; codec e DMA de áudio |
| P4 | Expansões observadas | leitor SD Realtek `10ec:525a`; SMBus `8086:1c22`; Bluetooth USB ainda não identificado | Enumerar e implementar apenas quando identificador/requisito de produto estiver confirmado |

Estado atual: PCI identifica os controladores da matriz; AHCI de boot só foi
exercitado com QEMU `8086:2922`; Ethernet funcional só com Intel 82574L QEMU
`8086:10d3`; framebuffer recebido do Limine não equivale a modesetting Intel.
Nenhum boot direto nem driver de I/O foi validado fisicamente. Portanto a
prioridade imediata de implementação é AHCI read-only para os dois IDs físicos,
seguida de Ethernet RTL8111 do NAS e I219-LM da estação, e então USB EHCI/xHCI.

Os IDs são uma matriz de detecção, não prova de compatibilidade. O caminho
AHCI agora permite IDENTIFY para os Intel alvo, mas nenhuma execução física foi
validada. Cada driver precisa passar testes positivos, negativos e de falha de
I/O em QEMU e no hardware antes de ser declarado suportado.

O kernel identifica `10ec:8168` (Realtek RTL8111 do servidor), `8086:15d7`
(Intel I219-LM local) e `8086:10d3` (Intel 82574L/QEMU). O driver
`src/arch/x86_64/e1000.rs` inicializa somente `8086:10d3`, transmite/recebe
quadros em polling e obtém DHCPv4 no QEMU com backend user-mode. Isso comprova
somente a NIC virtual QEMU; hardware físico, TCP, DNS e acesso geral à internet
seguem pendentes.

Modelo host de isolamento (2026-11-15): dois `AddressSpace` com a mesma VA user
resolvem frames distintos por CR3 e liberam suas tabelas separadamente. O
Hermione também tem teste host de fechar e remontar uma imagem para verificar
persistência. São validações de modelo host; ainda faltam persistência de
contas/processos no kernel e boot físico. RTL8111/I219-LM estão identificadas;
drivers Ethernet físicos RTL8111/I219, TCP, DNS e serviços de rede continuam
pendentes. O polling QEMU está documentado acima.

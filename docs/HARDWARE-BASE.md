# Hardware de referência do kernel

Inventário somente leitura coletado nesta máquina e em `nas-wifi` em
14/11/2026. Os IDs PCI orientam detecção e testes. Não acessar nem alterar
discos reais durante o desenvolvimento; testes de bloco usam imagens
descartáveis no QEMU.

| Classe | Máquina de desenvolvimento | Servidor NAS |
| --- | --- | --- |
| Armazenamento | SATA Intel RST/AHCI `8086:282a`; SSD SATA de 256 GB; leitor SD Realtek `10ec:525a`; cartão SD MMC | SATA AHCI Intel `8086:1c02`; discos SATA de boot, dados e membro ZFS |
| Rede cabeada | Intel I219-LM `8086:15d7` | Realtek RTL8111 `10ec:8168` |
| Wi-Fi | Intel Wireless 8265 `8086:24fd` | Não detectado no inventário PCI |
| USB | Intel xHCI `8086:9d2f` | Intel EHCI `8086:1c2d` e `8086:1c26` |
| Vídeo | Intel HD Graphics 520 `8086:1916` | Intel integrada `8086:0152` |
| Áudio | Intel HD Audio `8086:9d70` | Intel HD Audio `8086:1c20` |

O NAS não tem `lsusb` instalado; controladores PCI e dispositivos USB em sysfs
foram consultados. O inventário não significa que o CarlioOS já tenha esses
drivers: hoje ambos os sistemas dependem dos drivers do Linux.

## Drivers necessários

| Driver | Hardware base | Estado | Etapas principais |
| --- | --- | --- | --- |
| VGA compatível | VGA legado em QEMU | Detecção PCI parcial; driver não implementado | framebuffer/texto, detecção de modo, fallback serial |
| Intel iGPU | `8086:1916` local; `8086:0152` NAS | Detecção PCI parcial; driver não implementado | framebuffer inicial; aceleração fica posterior |
| SATA AHCI | `8086:282a` local; `8086:1c02` NAS; QEMU `8086:2922` | Driver DMA experimental apenas no QEMU (`8086:2922`); leitura/escrita em imagem descartável | Validar controladores físicos, erros e timeout; manter fallback sem disco |
| USB | xHCI `8086:9d2f` local; EHCI `8086:1c2d/1c26` NAS | Detecção PCI parcial; driver não implementado | enumeração, teclado/armazenamento, reset e timeout |
| Bluetooth | Adaptador USB local ainda não identificado; NAS sem adaptador PCI detectado | Não implementado | HCI sobre USB, firmware e descoberta de controlador |
| Ethernet | Intel I219-LM `8086:15d7`; Realtek RTL8111 `10ec:8168` | Detecção PCI parcial; driver não implementado | TX/RX DMA, interrupções/polling, link e falha de rede |
| Wi-Fi | Intel 8265 `8086:24fd` local | Detecção PCI parcial; driver não implementado | firmware, associação, autenticação e gestão de energia |
| Áudio HDA | `8086:9d70` local; `8086:1c20` NAS | Não implementado | PCI, codec, buffers DMA e saída básica |

Ordem de implementação: boot/mapa de memória/HHDM; PCI e AHCI; VGA e framebuffer;
xHCI/EHCI; Ethernet; Bluetooth; Wi-Fi; HDA. O kernel deve continuar headless
quando vídeo, rede ou áudio falharem.

Os IDs são uma matriz de detecção, não prova de compatibilidade. Cada driver
precisa passar testes positivos, negativos e de falha de I/O em QEMU antes de
ser declarado suportado.

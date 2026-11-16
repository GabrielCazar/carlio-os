# Incremento de vídeo — framebuffer e texto VGA compatível

**Estado:** base RGB de framebuffer validada em QEMU; IDs Intel HD Graphics 520
(`8086:1916`) e Ivy Bridge GT1 (`8086:0152`) reconhecidos. Não há driver nativo
Intel de modesetting nem controle direto dos conectores VGA, HDMI ou DP.

## Implementação

`src/video.rs` adiciona validação de layout, formato RGB de 24/32 bpp, pitch,
capacidade do buffer, escrita de pixel, recorte de retângulos e um buffer de
células de texto VGA com rolagem. Tudo usa memória fornecida pelo chamador e
não requer heap.

O boot solicita framebuffer ao Limine, valida dimensões, modelo RGB, pitch e
limite de 128 MiB antes de desenhar faixas de teste. O protocolo fornece o
endereço e formato da tela; o firmware/bootloader seleciona e inicializa o modo.
Assim, a mesma saída pode aparecer pelo conector que o controlador gráfico já
encaminhou, sem o kernel identificar se é VGA, HDMI ou DisplayPort. A
[especificação Limine](https://github.com/limine-bootloader/limine-protocol/blob/trunk/PROTOCOL.md#framebuffer-feature)
descreve o framebuffer, máscaras RGB e EDID.

O buffer VGA implementa células em memória, mas ainda não escreve diretamente
na memória/registradores VGA do hardware. Não há detecção de EDID/hotplug,
programação de modo, treinamento de link HDMI/DP, aceleração nem driver nativo
para as iGPUs Intel `8086:1916` e `8086:0152`.

## Validação

69 testes da biblioteca, 1 do `carlio-logs` e 3 do `herm-fs` passaram após a
classificação dos modelos Intel. A ISO validou framebuffer RGB em BIOS e UEFI
no QEMU TCG local e QEMU/KVM no NAS.
O smoke exige o marcador `Framebuffer Limine: RGB ativo; renderização de pixels
validada.`. Essa prova confirma que o kernel escreveu no framebuffer; não prova
saída física em VGA/HDMI/DisplayPort no servidor.

Evidências finais: [host local](logs/fase-video-intel-host-20261115-155727.txt),
[BIOS local](logs/fase-video-intel-qemu-bios-20261115-155727.log),
[UEFI local](logs/fase-video-intel-qemu-uefi-20261115-155727.log),
[execução NAS](logs/fase-video-intel-nas-20261115-155750.txt),
[BIOS NAS](logs/fase-video-intel-nas-20261115-155750-qemu-kvm-bios.log) e
[UEFI NAS](logs/fase-video-intel-nas-20261115-155750-qemu-kvm-uefi.log).

## Integração `embedded-graphics` (2026-11-15)

A feature Cargo opcional `graphics` implementa `embedded_graphics::DrawTarget`
e `OriginDimensions` sobre o framebuffer validado. O teste verifica a
conversão RGB888 e posição do pixel. Isso fornece um backend 2D por CPU; não
adiciona modos Intel, aceleração nem controle de conectores.

`cargo test --all-targets --features graphics` passou 74 testes (70 biblioteca,
1 carlio-logs e 3 herm-fs); ver [log com a feature](logs/fase-status-plugins-feature-host-20261115-1620.txt).
`scripts/test-local-iso.sh` passou BIOS/UEFI em QEMU TCG e
`scripts/test-nas.sh` passou a suíte host (69 biblioteca, 1 logs, 3 herm-fs) e
BIOS/UEFI em QEMU/KVM. Evidências:
[BIOS local](logs/fase-status-plugins-qemu-bios-20261115-161757.log),
[UEFI local](logs/fase-status-plugins-qemu-uefi-20261115-161757.log),
[resultado NAS](logs/fase-status-plugins-nas-20261115-161815.txt),
[BIOS NAS](logs/fase-status-plugins-nas-20261115-161815-qemu-kvm-bios.log) e
[UEFI NAS](logs/fase-status-plugins-nas-20261115-161815-qemu-kvm-uefi.log).

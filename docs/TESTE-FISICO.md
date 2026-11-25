# Teste físico da ISO

Imagem experimental de boot gerada em 2026-11-16:

- Arquivo: `build/carlio-os.iso`
- Tamanho: 4.528.128 bytes (4,32 MiB)
- SHA-256: `adc2b48ac52a8ca738ef981a55291e7c8bf2d7b3e0ffdd89505992cc54610be8`
- Smoke automatizado: BIOS/UEFI no QEMU TCG e KVM; 129 testes host.
- O pendrive removível `Flash Disk` recebeu os 4.528.128 bytes da ISO por
  `dd` em `/dev/sdb`; o comando terminou com sucesso. `lsblk` mostrou as três
  partições híbridas de boot (34 KiB, 3 MiB e 1,2 MiB). Ainda não houve leitura
  de volta nem boot físico; o espaço restante não foi particionado.

## Gravar em pendrive

A gravação apaga o conteúdo do dispositivo escolhido. Confirme modelo e
capacidade antes de usar o caminho do disco inteiro, por exemplo `/dev/sdX`, e
desmonte as partições desse pendrive. Não use o disco do sistema.

```sh
lsblk -o NAME,SIZE,MODEL,TRAN,TYPE,MOUNTPOINTS
sudo umount /dev/sdX1  # repita para cada partição montada do pendrive
sudo dd if=build/carlio-os.iso of=/dev/sdX bs=4M status=progress conv=fsync
sync
```

Substitua `/dev/sdX` pelo dispositivo confirmado. O arquivo é uma ISO híbrida
BIOS/UEFI; o pendrive de 8 GiB serve como mídia, mas a ISO não instala nem
particiona automaticamente o restante dele. Secure Boot não foi validado e
pode precisar ser desativado para iniciar o Limine. A escrita exige acesso de
administrador ao dispositivo; não tente executá-la dentro de um container sem
pass-through de `/dev`.

## Limites de segurança do teste

Gere a imagem de diagnóstico com `CARLIO_PHYSICAL_TEST=1 ./scripts/build-iso.sh`.
A ISO resultante fica em `build/carlio-os-physical-test.iso` e emite
automaticamente o diagnóstico e a lista de erros do kernel no boot, entre
`CARLIO-PHYSICAL-REPORT BEGIN` e `CARLIO-PHYSICAL-REPORT END`. O perfil
dispensa entrada serial durante o boot e desativa a abertura das reservas
persistentes AHCI e a inicialização do swap.
Capture a saída pela serial para manter os resultados fora do equipamento testado.
O kernel ainda precisa de um driver USB mass-storage e de um caminho de escrita
para devolver o relatório no pendrive. Não habilite o teste
`storage-test` em hardware físico; ele grava setores no dispositivo AHCI que o
driver selecionar. Não instale o CarlioOS, não conecte discos com dados
importantes e não trate o resultado como validação de resistência a ataques.
Ainda faltam drivers físicos, log persistente, recuperação após perda de energia
e proteção contra falhas físicas de memória. A imagem não deve receber dados
reais.

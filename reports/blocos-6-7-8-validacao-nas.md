# Blocos 6, 7 e 8 · validação no NAS

O ensaio usou QEMU/KVM no NAS Manjaro, com quatro CPUs virtuais, 512 MiB
e imagens descartáveis. A ISO padrão foi usada para boot. Uma ISO
instrumentada habilitou os comandos de escrita e falha do kernel.

| Prova | BIOS | UEFI |
| --- | --- | --- |
| Boot da ISO e diagnóstico serial sem display | Passou | Passou |
| Boot direto da imagem raw criada pelo preparador | Passou | Passou |
| Execução ELF ext4 em CPL3, VFS, DAC e `mmap` privado | Passou | Passou |
| Leitura de arquivo FAT32 em CPL3 | Passou | Passou |
| Escrita, remontagem e recuperação do overlay VFS | Passou | Passou |
| Banco de contas persistente e autenticação após remontagem | Passou | Passou |
| Swap cifrado, flush e recuperação do log após escrita parcial | Passou | Passou |
| Recuperação de metadados nativos ext4 após corte simulado | Passou | Passou |
| Recuperação de metadados nativos FAT32 após corte simulado | Passou | Passou |
| AHCI IDENTIFY, escrita, leitura, erro ATA e scrub Hermione | Passou | Passou |
| AHCI timeout, COMRESET e nova leitura | Passou | Passou |
| Retirada SATA com comando pendente e controlador presente | Passou | Passou |

Overlay, contas e swap passaram na repetição com a ISO instrumentada. A
primeira tentativa usou a ISO padrão, na qual os comandos destrutivos de
teste ficam desativados. Timeout e retirada passaram na repetição com o
QEMU 11.1.2 instrumentado. Nessa configuração, o autoteste de tabelas de
páginas do kernel reportou falha nos dois firmwares. O carregamento em
CPL3 e a preempção entre espaços passaram na configuração padrão de boot.

O disco QCOW2 esparso de 2 TiB passou no ensaio de limite AHCI. DHCPv4 e
ARP responderam nos dois firmwares. A consulta DNS A ficou sem resposta
válida. O teste do binário real do Ubuntu em BIOS chegou ao carregador,
que falhou ao mapear a glibc.

Na estação, 208 testes Rust passaram, assim como Clippy da biblioteca.
A prévia da instalação virtual preservou o destino ausente. O
preparador recusou hash incorreto, confirmação divergente e destino
existente. A imagem criada foi comparada byte a byte com a ISO.

O preparador exige um SHA-256 obtido de fonte autenticada, seguindo o
princípio de verificação de mídia do
[AlmaLinux](https://wiki.almalinux.org/documentation/installation-guide).
Cada imagem ocupa um novo arquivo, preservando a versão anterior para
seleção manual na VM. A recuperação transacional do sistema, como a
oferecida pelo [Snapper no openSUSE](https://doc.opensuse.org/documentation/leap/reference/single-html/book-reference/),
continua pendente.

Os três blocos estão em andamento. Faltam login gráfico e agentes,
instalação física e atualização transacional, Secure Boot, e execução
de aplicativos Windows. A validação foi feita no guest, sem alterar
partições reais do NAS.

Os logs brutos foram copiados para `target/bloco678-tmp/nas-logs/` no
workspace, incluindo as primeiras tentativas e os retestes.

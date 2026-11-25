# Configuração e operação

## Opções usadas hoje

| Variável | Efeito |
| --- | --- |
| `CARLIO_TEST_PHASE` | Prefixo da fase usado nos nomes dos logs dos testes locais/NAS. |
| `CARLIO_TEST_ISO` | Imagem ISO usada por `scripts/test-nas.sh`. O padrão é `build/carlio-os.iso`. |
| `CARLIO_TEST_OVMF` | Caminho para `OVMF_CODE` quando o harness testa UEFI. |
| `CARLIO_STORAGE_TEST=1` | Acrescenta teste Hermione/AHCI em disco descartável ao harness NAS. |
| `CARLIO_CRASH_TEST=1` | Ativa comandos de injeção de falhas de kernel na build de diagnóstico. |
| `RUSTUP_HOME`, `CARGO_HOME` | Diretórios de toolchain/cache usados pelo build bare-metal. |

O arquivo [`teste-local.env.example`](teste-local.env.example) contém valores de
exemplo. Ele não é carregado automaticamente; use `source` antes de chamar o
script. Não ponha senhas ou chaves nesses arquivos.

## Valores definidos no código

- Boot QEMU: 512 MiB RAM, máquina Q35 e serial COM1.
- Kernel atual: x86_64; alvo bare-metal `x86_64-unknown-none`.
- Idioma próprio do boot/terminal: pt_BR.
- Volume Hermione v2 de teste: setores de 512 bytes, sem cifra e sem montagem
  automática. CRC32 não autentica dados.

## Configuração ainda planejada

Não há ainda arquivo persistente de configuração para interfaces de rede,
usuários, montagem de FAT32/ext4, swap, Plasma/SDDM, agentes IA ou instalação.
O layout futuro prevê `/configuracoes` e `/configuracoes/servicos`; esses
caminhos atualmente existem apenas no VFS host de teste.

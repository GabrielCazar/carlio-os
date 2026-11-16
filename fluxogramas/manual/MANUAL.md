# Manual inicial do CarlioOS

## Estado do sistema

A ISO atual inicia um kernel experimental de diagnóstico. Ela não instala o
sistema, não inicia uma userland Arch, não tem login gráfico e não deve receber
dados reais. Os testes de armazenamento escrevem somente em imagens descartáveis
do QEMU.

## Construir e testar a ISO

No Arch Linux de desenvolvimento, instale as dependências documentadas no
README. Dentro do repositório:

```sh
scripts/test-local-iso.sh
```

O script executa `cargo fmt --check`, os testes host, cria `build/carlio-os.iso`
e inicia QEMU TCG para validar os comandos seriais em BIOS e, quando OVMF está
instalado, UEFI. Cada execução salva saídas em `reports/logs/`.

Para uma sessão serial manual:

```sh
scripts/test-qemu.sh
```

No QEMU, o terminal inicial aceita `ajuda`, `sobre`, `usuario`, `permissoes`,
`erros`, `limpar` e `parar`. O terminal atual executa no ring 0 e serve apenas
para diagnóstico.

## Teste Hermione em mídia descartável

O comando `fs-teste` só existe em build com `storage-test`. O harness
`scripts/test-ahci-qemu.py` cria uma imagem temporária de 16 MiB, inicia QEMU e
remove a imagem ao terminar. Não conecte unidades físicas a este teste. O fluxo
não testa os discos do NAS nem o driver AHCI físico.

## Próximos recursos

Antes de uma ISO instalável para o NAS, ainda são necessários journal replayável,
cifra e KDF, recuperação de corte de energia, timeouts/remoção AHCI, driver
Realtek RTL8111, swap, montagem normal e instalador com proteção contra
sobrescrita. Veja o [roadmap](../../docs/ROADMAP.md) e o
[relatório da fase 2](../../reports/fase-2-armazenamento.md).

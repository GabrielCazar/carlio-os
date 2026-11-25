# Lockdown do kernel

A linha de comando do Limine aceita `lockdown=none`, `lockdown=integrity`
ou `lockdown=confidentiality`. Sem parâmetro e sem mínimo compilado, o padrão
é `none`. Quando há mais de uma opção, vence a mais restritiva. Uma opção
inválida seleciona `confidentiality`. O kernel fixa o nível no boot e o
exibe no console serial. `CARLIO_LOCKDOWN_FLOOR` define o mínimo compilado e
uma opção de boot só pode aumentá-lo.

| Nível | Restrições |
| --- | --- |
| `none` | A ABI e o diagnóstico seguem seu comportamento padrão. |
| `integrity` | Retorna `EPERM` inclusive para UID 0 ao acessar interfaces de memória física e portas, ou ao solicitar I/O direto, módulos, kexec e suspensão em disco. Bloqueia comandos de teste de falha e disco no console. |
| `confidentiality` | Soma o bloqueio de interfaces de depuração, leitura de símbolos e dumps de memória. Suprime registradores, detalhes de panic e endereços da captura de exceções fatais no serial. |

A ABI intercepta as chamadas de módulo e kexec. O kernel tem drivers
compilados na imagem. O carregador ELF de módulos existente ainda não está
ligado a um fluxo dinâmico com assinatura verificada. A política do executável de serviço do dircache é independente.
Os bloqueios são registrados no coletor de erros do kernel.

## Imagem de teste

```sh
CARLIO_LOCKDOWN=confidentiality ./scripts/build-iso.sh
```

## Integração com UEFI Secure Boot

```sh
CARLIO_LOCKDOWN=integrity CARLIO_SECURE_BOOT_STAGE=1 ./scripts/build-iso.sh
```

O estágio UEFI em `build/carlio-os-lockdown-integrity-uefi-stage/` contém
o hash BLAKE2b do kernel no `limine.conf` e o hash da configuração inscrito
no carregador EFI. O kernel recebe também um mínimo compilado. Assine
`EFI/BOOT/BOOTX64.EFI` **após** a inscrição do hash, com a chave de assinatura
de boot do ambiente. Instale a árvore na ESP e faça o boot com a respectiva
chave pública confiável no firmware e Secure Boot ativo. Mantenha BIOS/CSM
desabilitado em instalações que dependam dessa cadeia de confiança.

A ISO normal permite testes BIOS e UEFI. A assinatura e o provisionamento
das chaves do firmware são etapas do ambiente de implantação. O kernel
seleciona o nível pelo mínimo compilado e pela linha de comando verificada
na cadeia de boot. Ele ainda não consulta o estado Secure Boot via UEFI.

Referências: [protocolo Limine](https://github.com/limine-bootloader/limine-protocol/blob/trunk/PROTOCOL.md)
e [cadeia de confiança do Limine 12](https://github.com/limine-bootloader/limine/blob/v12.x/USAGE.md).

## Validação

232 testes passaram. Os perfis `integrity` e `confidentiality` iniciaram
em QEMU TCG com BIOS e UEFI. A inscrição do hash da configuração no
Limine foi verificada. O boot com Secure Boot ativo segue pendente.

# Registro para aplicações Windows no CarlioOS

A arquitetura prevê aplicações PE32+ em CPL3 no CarlioOS, com as chamadas de
Registro encaminhadas para um serviço em uma VM dedicada.

```text
Aplicação PE32+ → camada Win32 → broker do CarlioOS → canal privado → VM de Registro
                                       ↑
                          credenciais obtidas do kernel
```

O executável `carlio-registry-guest` implementa o serviço convidado pelo stdin
e stdout. Ele pode ser executado em um convidado Linux compatível com o binário.
Cada processo atende um hive volátil. O binário também roda localmente para
testar o serviço e o enquadramento do canal.

## Protocolo local v1

Todos os comprimentos de 16 bits e os dados `REG_DWORD` usam little-endian.

- Pedido: comprimento de 16 bits seguido de até 229 bytes. O corpo começa com
  cinco bytes: versão (1), operação (1 consulta, 2 gravação, 3 remoção),
  comprimento do caminho, comprimento do nome e tipo.
- Depois do cabeçalho vêm o caminho, o nome e, em gravações, os dados. Para
  consulta e remoção, use tipo zero e termine o pedido após o nome.
- Caminho e nome juntos têm até 96 bytes ASCII imprimíveis, comparados sem
  distinção de maiúsculas. O caminho tem componentes não vazios separados por
  barra invertida. Um nome vazio identifica o valor padrão do caminho.
- Tipos disponíveis: `REG_BINARY` (3, até 128 bytes) e `REG_DWORD` (4, quatro
  bytes). Cada hive comporta 16 valores e admite substituição quando cheio.
- Resposta: um byte de status (0 sucesso, 1 pedido inválido, 2 valor ausente,
  3 hive cheio), um byte de tipo e comprimento de 16 bits, seguido dos dados
  de consulta em caso de sucesso. Os campos de tipo e comprimento são zero
  nas demais respostas.
- O serviço encerra o canal em caso de enquadramento truncado ou acima do limite.
  Um pedido inteiramente recebido com corpo inválido gera status 1.

Consulte [tipos de valores do Win32](https://learn.microsoft.com/en-us/windows/win32/sysinfo/registry-value-types)
e [semântica de consulta](https://learn.microsoft.com/windows/win32/api/winreg/nf-winreg-reggetvaluew).

## Fronteira de segurança e integração

O broker de referência vincula o canal e o hive ao UID informado pelo
supervisor. Ele confere o UID vigente e os direitos de consulta ou gravação
em cada operação, valida a resposta inteira antes de copiar dados e encerra
a sessão se o canal falhar ou devolver uma resposta inválida. Cada sessão
processa um pedido de cada vez. A gravação pode ter sido aplicada antes de
uma falha no canal. O broker não repete operações automaticamente.

Em Linux, `RegistryVm` inicia um QEMU TCG com uma vCPU, 256 MiB de RAM e
um socket Unix privado ligado à porta serial. A VM roda sem rede, discos
de dados, monitor ou pastas compartilhadas. O supervisor limita o boot a
30 s e cada troca de mensagens a 2 s. Use binário QEMU e imagens de boot
confiáveis. Cada sessão possui uma VM e um hive volátil. O supervisor
chamador controla quantas sessões podem coexistir.

`RegistrySession` implementa a semântica inicial de `RegQueryValueExW`,
`RegSetValueExW` e `RegDeleteValueW` para os tipos suportados. A consulta
aceita destino ausente para obter o tamanho e devolve `ERROR_MORE_DATA`
quando o destino é insuficiente. A entrada usa unidades UTF-16 limitadas a
ASCII imprimível nesta versão.

Para execução no CarlioOS, faltam um backend de virtualização do próprio
kernel, a obtenção das credenciais e dos direitos na entrada CPL3, as
tabelas de `HKEY`, a validação de ponteiros do usuário e a resolução dos
imports Win32 pelo carregador PE. O supervisor QEMU roda no Linux e a
camada de valores opera sobre slices Rust. Persistência, Unicode completo
e gerenciamento de chaves e hives compartilhados seguem pendentes.

## Validação

```sh
cargo test --offline --lib userland::windows
cargo test --offline --bin carlio-registry-guest
cargo check --offline --bin carlio-kernel --target x86_64-unknown-none
```

O teste QEMU requer Linux x86_64, QEMU, cpio, a libc estática e uma imagem
Linux x86_64 com devtmpfs, driver 8250 e suporte a initramfs incorporados.
Defina `CARLIO_REGISTRY_KERNEL` com o caminho dessa imagem antes de rodar.

```sh
cargo rustc --offline --release --bin carlio-registry-guest -- -C target-feature=+crt-static
guest_dir=$(mktemp -d)
mkdir -p "$guest_dir/root/dev"
cp target/release/carlio-registry-guest "$guest_dir/root/init"
(cd "$guest_dir/root" && printf '%s\n' dev init | cpio -o -H newc > "$guest_dir/guest.cpio")
CARLIO_REGISTRY_INITRAMFS="$guest_dir/guest.cpio" \
  cargo test --offline --test registry_vm -- --ignored
```

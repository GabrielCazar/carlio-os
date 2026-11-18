# Protocolo opt-in de dircache — v1

Estado: especificação proposta; o kernel ainda não fornece o canal. O canal
preferido é um endpoint AF_UNIX `SOCK_SEQPACKET` em
`/run/carlio/dircache.sock`, com ativação apenas pelo perfil de instalação
`dircache=enabled`. `AF_CARLIO` pode ser acrescentado como transporte futuro,
sem mudar os frames deste protocolo.

## Acesso e ciclo de vida

Desabilitado por padrão. A ativação exige configuração explícita, capability
`CAP_DAC_OVERRIDE` para registrar o backend, política LSM permitindo o
registro e, quando verified boot estiver ativo, assinatura do executável
verificada contra a política local. Nenhuma credencial de assinatura é
embutida no protocolo. O registro é atômico; falha não deixa backend parcial.
Cada cliente e backend tem identidade de processo e geração. Encerramento do
backend invalida a geração e acorda consultas pendentes com erro tipado.

## Frames v1

Inteiros little-endian; comprimento total limitado a 64 KiB; versão e tamanho
sempre explícitos. Cada consulta carrega `request_id:u64`, `generation:u64`,
`name_len:u32`, `flags:u32` e bytes UTF-8 do nome. Resposta carrega o mesmo
`request_id`, status enumerado, `ttl_ms:u32`, tamanho e payload UTF-8. Payload
de resposta contém entradas de diretório (nome, uid/gid numéricos, grupos
opcionais) e nunca contém ponteiros, caminhos locais arbitrários ou tokens.
Timeout é limitado pela política de instalação; timeout, frame malformado,
versão desconhecida e backend ausente falham fechados, sem fallback para
identidade privilegiada.

Toda inscrição, remoção, consulta, resposta, timeout e erro gera evento no
`/dev/carlio/audit`. Logs não incluem tokens nem conteúdo sensível. O daemon
NSS do userland conecta-se ao endpoint e traduz `getpwnam`/`getgrnam`; libc do
kernel não faz RPC implícito.

## Segurança e estabilidade

Clientes só consultam; apenas o principal autorizado registra/substitui o
backend. Não confiar em uid/gid retornados para conceder capability. Limites
de concorrência, TTL máximo e tamanho são impostos pelo kernel. v1 é estável
após implementação e conformidade; extensões usam flags negociadas. O canal
permanece indisponível até testes positivos, negativos, falha de backend e
auditoria em QEMU e NAS/KVM.

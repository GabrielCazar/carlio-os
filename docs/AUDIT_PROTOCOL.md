# Barramento de auditoria — v1

Estado: especificação proposta; `/dev/carlio/audit` ainda não existe. O
consumidor abre o device somente após ativação explícita na instalação. A
política padrão nega leitura e emissão por userland não autorizado. Leitores
exigem `CAP_AUDIT_WRITE` e autorização LSM. Eventos são somente leitura e não
podem bloquear caminhos de syscall: buffer cheio incrementa contador de perda
e registra uma indicação de overflow na próxima oportunidade.

## Registro binário

Cabeçalho little-endian de tamanho fixo: `version:u16`, `kind:u16`,
`length:u32`, `sequence:u64`, `monotonic_ns:u64`, `pid:u64`, `uid:u32`,
`gid:u32`, `result:i32`, `flags:u32`; payload limitado a 4 KiB. Campos
ausentes são zero e indicados em `flags`. Sequência identifica lacunas. Versão
desconhecida deve ser ignorada pelo leitor; tamanho inválido é rejeitado.

Tipos mínimos: `ChannelRegister`, `ChannelUnregister`, `DirectoryLookup`,
`SandboxDenied`, `CapabilityDenied`, `LsmDenied`, `SocketBind`, `SocketConnect`,
`Exec`, `CredentialChange`, `AuditOverflow`. Cada tentativa de registro e
consulta de dircache, inclusive negada/falha, emite evento. Sandbox retorna
`EPERM` e segue o processo.

## Privacidade e entrega

Não registrar conteúdo de buffers, senhas, chaves, tokens, payload de pacotes
ou caminhos completos por padrão. Identificadores de objeto podem ser
hashes com salt efêmero de boot. Leitura usa fila bounded, ordem por sequência
e timeout; evento perdido nunca pode alterar a decisão de segurança. Não há
promessa de persistência após reboot. Integridade criptográfica, exportação e
rotação pertencem ao consumidor/instalador, não ao device v1.

Ativação exige config de instalação, capability e autorização LSM; assinatura
do binário também é exigida quando verified boot estiver ativo. A interface
fica desabilitada por padrão e é auditada sem recursão: o próprio emissor não
gera um evento por leitura de evento.

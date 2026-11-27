# CarlioOS kernel ABI — v1.0.0

Estado: contrato versionado **congelado para a ABI v1**. Congelar a forma e a
semântica não declara que uma chamada já está implementada. “Pendente” exige
execução e testes antes de ser anunciada como disponível. Mudanças incompatíveis
exigem ABI v2; extensões compatíveis ficam reservadas a flags, opções e novas
chamadas. Alvo: ELF x86-64, chamada `SYSCALL`, `RAX` como número/retorno e
`RDI, RSI, RDX, R10, R8, R9` como argumentos. Erros retornam `-errno` em `RAX`.

| Syscall x86-64 | Semântica contratada | Estabilidade v1 | Estado no kernel |
| --- | --- | --- | --- |
| `openat` (257) | Abre relativo a dirfd; flags/modo Linux x86-64 | Congelada | Parcial; VFS inline, O_CREAT/O_EXCL e caminhos relativos sob `/` |
| `read` (0), `write` (1) | I/O por fd; retorno de bytes, interrupção/EOF conforme POSIX-like | Congelada | Parcial. Console, VFS e TCP IPv4. O escalonador bloqueia a tarefa quando o socket TCP bloqueante aguarda bytes ou espaço de envio |
| `fstat` (5) | Preenche `struct stat` x86-64 a partir do descritor aberto | Extensão v1 | Parcial. Arquivos no VFS e descritores do console, sem timestamps reais |
| `access` (21) | Verifica F/R/W/X pela política POSIX do VFS | Extensão v1 | Parcial; IDs do contexto atual, cwd `/` |
| `getcwd` (79) | Copia o cwd NUL-terminated e retorna comprimento incluindo NUL | Extensão v1 | Parcial; cwd fixo `/`, sem `chdir` |
| `newfstatat` (262) | Metadados de caminho relativo a dirfd com flags Linux | Extensão v1 | Parcial. Cwd `/`, caminhos absolutos ou relativos ao cwd e `AT_EMPTY_PATH` para metadados por descritor |
| `mkdir` (83), `umask` (95) | Cria diretório e consulta/define máscara de permissões por processo | Extensão v1 | Parcial; VFS inline, cwd `/`, gates pendentes |
| `chmod` (90), `fchmod` (91), `fchmodat` (268) | Altera modo por caminho/descritor conforme proprietário/root e persiste rootfs | Extensão v1 | Parcial; VFS inline, cwd `/`, gates pendentes |
| `chown` (92), `fchown` (93), `lchown` (94), `fchownat` (260) | Altera UID/GID via caminho ou descritor; `-1` mantém campo | Extensão v1 | Parcial; somente root, VFS inline, sem links simbólicos |
| `unlink` (87), `rmdir` (84), `unlinkat` (263) | Remove arquivo/diretório conforme permissões do pai e persiste rootfs | Extensão v1 | Parcial; `AT_REMOVEDIR`, diretórios vazios, cwd `/`; arquivo aberto retorna `EBUSY` |
| `close` (3), `fsync` (74) | Fecha fd; `fsync` persiste dados e metadados do fd | Congelada | `close` parcial; `fsync` grava snapshot VFS inline para fd de arquivo |
| `renameat2` (316) | Renomeia entre dirfds; flags Linux, flags desconhecidas dão `EINVAL` | Congelada | Pendente |
| `statx` (332) | Metadados selecionados por mask; campos ausentes marcados no mask | Congelada | Parcial. Tipo, modo, links, UID/GID, inode e tamanho. Timestamps e alocação física ausentes da máscara |
| `mmap` (9), `munmap` (11), `mprotect` (10) | VM por processo; valida faixa/alinhamento e ownership; W^X | Congelada | mmap/munmap anônimo comprovados anteriormente; mprotect parcial em VMA anônima, validação suspensa nesta etapa |
| `clone` (56) | Cria processo/thread conforme flags; recursos compartilhados apenas quando pedidos | Congelada | Pendente como syscall geral |
| `futex` (202) | Espera/notify atômicos em palavra user; timeout monotônico | Congelada | Pendente |
| `execve` (59) | Prepara address space/stack novos, troca imagem do processo e CR3 no retorno; preserva PID/credenciais | Congelada | Integrado com limites fixos; sem irmãos ativos, argv/env UTF-8 limitados, sem gate QEMU/NAS |
| `poll` (7) | Multiplexação de descritores Linux | Extensão v1 | Consulta console, arquivos e sockets TCP. Timeout zero só consulta, positivo espera até o prazo monotônico e negativo espera por prontidão |
| `rt_sigaction` (13), `rt_sigprocmask` (14), `rt_sigreturn` (15), `tgkill` (234) | Entrega e máscara de sinais em retorno seguro | Congelada | `rt_sigaction` armazena as disposições padrão e ignorar para SIGPIPE por grupo de threads. Entrega, tratadores e máscara pendentes |
| `getrandom` (318) | CSPRNG do kernel; flags inválidas rejeitadas; falha explícita sem entropia | Congelada | Parcial. Usa o gerador de hardware x86 e retorna `EAGAIN` quando a fonte está indisponível. Espera e fallback pendentes |
| `clock_gettime` (228) | Relógios monotônico e realtime; estrutura compatível com Linux x86-64 | Congelada | CLOCK_MONOTONIC implementado em probe; CLOCK_REALTIME pendente |
| `epoll_create1` (291), `epoll_ctl` (233), `epoll_wait` (232) | Multiplexação por fd com eventos level-triggered v1 | Congelada | Pendente |
| `socket` (41), `bind` (49), `listen` (50), `accept4` (288), `connect` (42) | Sockets stream/datagram; flags de `accept4` validadas | Congelada | Bind IPv4, listen e accept/accept4 TCP. Connect TCP IPv4 usa a rota do DHCP e espera o handshake com timeout. Com SOCK_NONBLOCK retorna EINPROGRESS. Em TCP, accept/accept4 aguardam se o listener for bloqueante. Em listener não bloqueante retornam EAGAIN. SOCK_NONBLOCK em accept4 configura somente o fd aceito |
| `sendto` (44), `recvfrom` (45) | I/O em socket e endereço opcional | Extensão v1 | TCP conectado com endereço nulo. Aceita flags zero e MSG_NOSIGNAL no envio. Retorna EAGAIN quando a operação precisa esperar |
| `getsockname` (51) | Endereço local e porta IPv4 em `sockaddr_in` | Parcial | Consulta a tabela de sockets, inclusive porta efêmera de `bind(..., 0)` |
| `sendmsg` (46), `recvmsg` (47), `AF_UNIX`, `SCM_RIGHTS` | Stream/datagram local e transferência limitada de descritores | Congelada | Pendente |
| `AF_INET`, `AF_INET6` | Endereços IPv4/IPv6 Linux; erros de rede tipados | Congelada | IPv4 parcial; IPv6 pendente |
| `SO_REUSEPORT` (`setsockopt` 54) | Reuso controlado por protocolo/endereço/porta e credenciais | Congelada | Pendente |
| `sendfile` (40), `splice` (275) | Transferência entre fd sem cópia para user quando backend permitir | Congelada | Pendente |

## Resolução IPv4 para aplicações CarlIO OS

A extensão `carlio_resolve_ipv4` (`0x43410001`) recebe o ponteiro do nome em
`RDI`, seu comprimento em bytes em `RSI` e o ponteiro de saída em `RDX`.
O nome DNS ASCII pode conter de 1 a 253 bytes, mais um ponto final opcional.
Uma resposta A válida grava quatro octetos em ordem de rede e retorna zero.
A consulta usa o servidor da concessão DHCP vigente.

`EFAULT` indica memória de usuário inválida. `EINVAL` indica nome inválido.
`ENETUNREACH` indica ausência de concessão ou servidor DNS.
`ETIMEDOUT` indica ausência de resposta A válida. `EIO` indica falha do
controlador. `EAGAIN` indica controlador ocupado, intervalo entre consultas
ou indisponibilidade de entropia. Aplicações Arch e Ubuntu precisam usar
essa extensão diretamente. A integração com `getaddrinfo` permanece pendente.

## Capabilities v1

Capabilities pertencem ao contexto de credenciais do processo, são reduzidas
por `execve` conforme política e não são inferidas apenas de `uid=0`:

| Capability | Autoridade |
| --- | --- |
| `CAP_NET_BIND_SERVICE` | Bind a portas privilegiadas abaixo de 1024 |
| `CAP_DAC_OVERRIDE` | Bypass explícito de verificações DAC de arquivo, nunca de LSM |
| `CAP_AUDIT_WRITE` | Leitura do barramento estruturado de auditoria conforme ACL |

Sem capability ou hook LSM autorizado, a operação retorna `EPERM` e pode
produzir evento de auditoria sem encerrar o processo. A implementação e os
valores numéricos internos permanecem pendentes; não reutilizar os números
Linux de capability como garantia de suporte até o gate correspondente passar.

## Regras transversais

- Ponteiros user são faixas validadas no CR3 atual; comprimento zero, overflow,
  alinhamento e permissões são tratados explicitamente antes de tocar memória.
- `mprotect` não cria páginas executáveis graváveis; mapeamentos executáveis são
  imutáveis enquanto executáveis. O kernel mantém a política W^X.
- `clone`/`execve` não publicam tarefa/fd parcialmente inicializados. Falha
  reverte recursos e deixa o processo original consistente.
- `EPERM` de `carlio_sandbox` é auditado e não termina o processo. Falhas de
  memória user isolam a tarefa; não podem derrubar o kernel.
- `AF_CARLIO`, dircache e auditoria são extensões opt-in documentadas nos
  protocolos dedicados; não alteram semântica POSIX da v1.
- ABI Linux compatível não implica compatibilidade completa com glibc/musl ou
  Arch. Isso exige loader ELF dinâmico, VFS, threads, sinais, rede e testes de
  workload integrados.

## Mudança de versão

O número v1 e as semânticas acima não podem ser reutilizados com outro
comportamento. Nova chamada, flag ou campo extensível compatível pode ser
adicionado dentro da v1 se seu comportamento ausente for `ENOSYS`/`EOPNOTSUPP`
e a detecção for documentada. Remoção, renumeração, mudança de layout ou
semântica incompatível exige tabela separada e negociação de v2.

## Espera por descritores

Quando `accept`, `accept4`, `read` ou `write` precisam esperar em TCP
IPv4 bloqueante, o kernel preserva o frame CPL3, retira a tarefa da fila de
execução e despacha outra tarefa ou o laço ocioso. O PIT verifica prontidão
depois de processar a fila da Intel 82574L. A tarefa acorda na CPU que salvou
o frame e repete a operação. Isso permite observar mudanças de prontidão
ocorridas entre o despertar e o despacho. O caminho vale para `syscall` e
para o dispatcher de compatibilidade `int 0x80` do CarlioOS.

`poll` aceita até 256 descritores. O deadline é monotônico, com resolução
de 10 ms, e sobrevive a despertares em que a prontidão já mudou. O kernel
guarda uma cópia dos descritores durante a espera. A checagem de prontidão no
PIT usa essa cópia, e o resultado é escrito no buffer do usuário pela syscall
retomada. Sinais assíncronos, `epoll` e espera escalonada em `sendto` e
`recvfrom` permanecem pendentes.

## Processos supervisionados

O console carrega a imagem ELF com a identidade da sessão autenticada.
O processo começa suspenso e só entra na fila da CPU secundária depois da
instalação do frame e do vínculo com a VM. Os três descritores seriais padrão ficam fechados para reservar a UART ao
console. O serviço pode abrir arquivos de log pelo VFS com a própria identidade.

Se o processo estiver pronto ou bloqueado, a parada o remove do escalonador.
Se estiver em execução, o timer local o interrompe na fronteira de CPL3.
O supervisor só libera os recursos depois que a CPU secundária confirmou
o retorno à pilha ociosa e ao CR3 do kernel. A coleta encerra descritores,
sockets e esperas pendentes e libera a VM e a pilha privilegiada.

O console aceita `servico reiniciar PID` para o UID proprietário ou para
o UID 0. O supervisor aguarda a coleta do PID anterior e carrega o mesmo
caminho ELF com os argumentos, UID, GID, grupos e umask originais. O
processo recebe um novo PID. Se a carga falhar depois da coleta, o comando
retorna o erro e o serviço fica parado.
## Espera com `ppoll`

A ABI x86-64 (syscall 271) valida o `timespec` de 64 bits e usa a mesma
fila do escalonador que `poll`. Um ponteiro nulo representa espera
indefinida. Um prazo zero consulta a prontidão imediatamente. Prazos
positivos são arredondados para cima e mantidos durante as reexecuções
da syscall. O prazo fornecido é somente entrada. A máscara de sinais
deve ser nula ou um conjunto vazio de oito bytes. Máscaras que pedem
bloqueio de sinais retornam `EOPNOTSUPP`.

## Criação de processo pela libc

`clone` (syscall 56) aceita a combinação usada pelo `fork` da glibc em
x86-64, `CLONE_CHILD_SETTID | CLONE_CHILD_CLEARTID | SIGCHLD`, com pilha
herdada e espaço de endereçamento privado. O kernel valida o campo de TID
gravável no filho e preenche o identificador antes de publicar a tarefa.
O pai mantém o conteúdo original. A variante usa a mesma herança de
descritores e coleta por `wait4` que `fork`. A liberação do espaço privado
descarta o TID junto com suas páginas. Compartilhamento de VM, futex de
saída e entrega de `SIGCHLD` seguem pendentes.

# Threat model — CarlioOS como servidor de diretório

## Ativos e fronteiras

Ativos: memória do kernel, credenciais e keyring, identidade de contas,
integridade do VFS, disponibilidade do processo pegasusDS, sockets e trilha de
auditoria. Fronteiras: processo ring 3 → syscall → kernel; socket de rede →
pilha de rede → processo; serviço pegasusDS → integração NSS/dircache; módulos
ring 0 → kernel (mesmo domínio de confiança).

## Adversários e ameaças

- Cliente remoto envia frames malformados, floods, conexões lentas e padrões de
  autenticação caros para esgotar CPU, memória, fd ou filas.
- Processo comprometido tenta escapar por ponteiros inválidos, corridas de VM,
  syscall não autorizada, escalada de capability, `ptrace` ou troca de uid/gid.
- Backend NSS/dircache falso ou reiniciado injeta identidades, respostas
  obsoletas ou bloqueia resolução de contas.
- Buffer de auditoria lotado ou consumidor malicioso tenta bloquear syscalls,
  ocultar eventos ou exfiltrar dados sensíveis.
- Módulo LKM malicioso tem privilégio ring 0 total; assinatura não o torna
  memory-safe nem isolado de outros módulos.

## Controles exigidos

Isolamento de CR3 e stacks RSP0; cópia user validada; W^X; limites por processo;
ownership/reaper robustos; capacidades por tarefa; whitelist seccomp-equivalente
com `EPERM` auditável; hooks LSM em `file_open`, `socket_bind`,
`socket_connect`, `task_alloc`, `ptrace`, `setuid` e `setgid`; keyring para
credenciais de vida curta; rate limiting/backpressure de rede; canais de
integração opt-in, autenticados e falha fechada; auditoria bounded que não
bloqueia enforcement. Cada controle requer casos positivos, negativos e de
falha em host e QEMU; caminhos remotos precisam também NAS/KVM.

## Riscos residuais e estado

Hoje a ABI é parcial, execução de processo continua em prova e o gate CPL3 nos
APs falha em BIOS/UEFI local com `#GP(0x18)`. A pilha TCP/IP não tem TCP,
accept/connect/recv, AF_UNIX, IPv6 ou epoll completos. Capabilities atuais são
principalmente modelos/políticas; seccomp, LSM e keyring não estão integrados.
Logo o kernel não está pronto para serviço exposto à rede. Tratar o host QEMU
como ambiente de teste sem dados reais até todos os gates de isolamento,
workload, rede e recuperação passarem.

# Relatório — Fase 3: segurança, contas e isolamento

### Correção — negação POSIX em syscall CPL3 — 2026-11-16

O probe de execução agora publica uma fixture com UID/GID 1000, mantendo o
arquivo `/programas/probe` executável e legível por `other`, e tenta abrir
`/programas/restrito`, pertencente a root com modo `0600`. O dispatcher aplica
o contexto imutável do processo ao `openat`; o teste exige retorno `EACCES` e
emite marcador serial. O smoke QEMU falha se esse marcador faltar. Isso prova
concessão e negação para `openat` no fixture CPL3, sem afirmar enforcement
geral para todos os processos nem política de inode persistente.

Validação: `scripts/test-local-iso.sh` passou `cargo fmt --check`, 156 testes
host e boots BIOS/UEFI QEMU TCG. `scripts/test-nas.sh` também passou 156 testes,
BIOS/UEFI QEMU/KVM e os probes de storage, com todos os artefatos remotos sob
`/tmp/carlioos-test-*`. Evidências: [host local](logs/fase-3-seguranca-20261116-host-20261116-194837.txt),
[BIOS local](logs/fase-3-seguranca-20261116-qemu-bios-20261116-194837.log),
[UEFI local](logs/fase-3-seguranca-20261116-qemu-uefi-20261116-194837.log),
[execução NAS](logs/fase-3-seguranca-storage-20261116-nas-20261116-194926.txt),
[BIOS NAS](logs/fase-3-seguranca-storage-20261116-nas-20261116-194926-qemu-kvm-bios.log)
e [UEFI NAS](logs/fase-3-seguranca-storage-20261116-nas-20261116-194926-qemu-kvm-uefi.log).

**Estado:** enforcement CPL3 demonstrado para leitura de arquivo inline, com
negação `0600`/`EACCES`; ainda é fixture de diagnóstico. Persistência/login,
auditoria durável de negações, backends mounted, processos gerais e proteção
criptográfica de contas permanecem pendentes.

## Contas persistentes em modelo host e identidade de processo (2026-11-15)

`userland::accounts::AccountDatabase` serializa/deserializa contas com checksum,
UID/GID, grupos suplementares e hash opaco; os dados persistem como arquivo no
volume Hermione e foram testados após desmontar/remontar. A verificação de hash
é delegada a `PasswordHashVerifier`; nenhum KDF foi implementado e hashes
seguros precisam ser fornecidos por um backend criptográfico real.

O scheduler agora carrega `ProcessSecurityContext` em cada processo e permite
obter `AccessContext` POSIX. A atribuição da pilha kernel e da identidade ocorre
ao publicar o processo, sob o mesmo lock. Persistência de contas foi validada
em host; ainda não existe login de boot conectado ao kernel, e a política VFS
não está ligada à entrada de syscall para processos gerais.

## Incremento de operações VFS host (2026-11-15)

`Vfs<N>` agora guarda até 16 KiB inline por arquivo e oferece
`read_file_as`/`write_file_as`. Ambas resolvem o caminho exigindo busca nos
diretórios ancestrais e verificam `r`/`w` pela identidade, grupo suplementar e
modo POSIX do arquivo. Escrita grande, leitura em diretório, buffer curto e
acesso sem permissão são recusados sem cópia parcial. `remove_file_as` verifica
`w+x` no pai, recusa diretórios e aplica o bit sticky; o slot do nó e o conteúdo
inline são limpos e o slot pode ser reutilizado.

Isto ainda é uma implementação host com capacidade fixa: não lê/grava em
BlockDevice/FAT32/Hermione, não é invocada por syscall do kernel, e não
substitui isolamento de processos, contas persistentes, ACL ou rede. Os testes
negativos novos cobrem permissões, tipo de nó e limite de arquivo.

Validação na revisão atual: 101 testes host locais e NAS; boots BIOS/UEFI local
e NAS/KVM passaram. O teste host desta API está em
[logs/fase3-5-incrementos-host-20261115-194208.txt](logs/fase3-5-incrementos-host-20261115-194208.txt)
e também integra a [suíte NAS](logs/fase3-5-incrementos-nas-20261115-194229.txt).
A revisão final elevou a suíte a 102 testes host, com as regras de remoção e
reutilização verificadas; logs: [host local](logs/fase3-5-fechamento-host-20261115-194459.txt)
e [host NAS](logs/fase3-5-fechamento-nas-20261115-194519.txt).

**Estado:** políticas POSIX/Linux ampliadas e ligadas a sessões/catálogo de
apps no VFS host; enforcement no kernel e processos gerais continuam pendentes.
Há uma prova controlada em ring 3 para validar rotinas, sem ABI de produção.

## Escopo

IDT/exceções, paginação, ring 3, scheduler, espaço de endereçamento por
processo, syscalls validadas, contas e permissões efetivamente aplicadas por
capability.

## Dependência da fase 6 — conexões remotas

O gerenciador de arquivos nativo planejado para a fase 6 terá conexões SSH,
SFTP/SCP e SMB. A fase 3 deve fornecer permissões por usuário/processo e
armazenamento seguro de chaves e credenciais, sem gravá-las em logs ou histórico;
essa persistência depende do volume protegido da fase 2. Autenticação e
autorização de cada compartilhamento precisam ser testadas. Essa integração
está planejada, não implementada.

## Critérios de aceite

Testes demonstram que processos não acessam memória nem recursos uns dos outros,
syscalls malformadas são rejeitadas, permissões negadas são auditadas e falhas
não comprometem outros processos. Revisão de segurança documentada antes de
usar dados reais.

## Estado comprovado

O shell e a política de VFS ainda não estão ligados a processos; existe somente
um probe temporário CPL3 validado em QEMU. Não há contas nem controle de acesso
efetivo no kernel. A política host abaixo é contrato, não fronteira de
segurança do sistema em boot.

## Incremento — permissões Unix/Linux no VFS

- `AccessContext` carrega UID, GID primário, até oito grupos suplementares e
  `umask`; as verificações selecionam owner/group/other como em POSIX.
- `check_access_as` e `authorize_execution_as` exigem busca (`x`) em cada
  diretório percorrido, inclusive nos componentes `.` e `..`. Root ignora
  leitura/escrita, mas um arquivo precisa ter algum bit de execução para ser
  executado.
- Criação aplica `umask`, herda o GID de diretório setgid e mantém setgid em
  subdiretórios. Bits setuid/setgid em arquivos criados por usuário sem
  privilégio são limpos. `set_owner` é restrito a UID 0; `set_mode` permite
  bits especiais POSIX ao dono/root. Metadados expõem tipo, UID, GID e modo.
- `CredentialVerifier` pode fornecer até oito grupos suplementares para a
  `Session`; o `AppCatalog` passa esses grupos à autorização antes do loader.
  O backend de contas ainda é injetável e não persiste usuários/senhas.
- Host: 53 testes passaram; testes negativos cobrem atravessar diretório sem
  busca, execução negada, alteração de owner sem root e acesso fora do grupo.
  Testes BIOS/UEFI passaram em QEMU local e NAS/KVM. A ISO continua inicializando
  somente o kernel de diagnóstico em ring 0; isso não testa enforcement da
  política no kernel.
- Evidências locais: [host](logs/fase-local-host-20261115-150133.txt),
  [BIOS](logs/fase-local-qemu-bios-20261115-150133.log) e
  [UEFI](logs/fase-local-qemu-uefi-20261115-150133.log).
- Evidências NAS/KVM: [execução](logs/fase-3-permissoes-final-nas-20261115-150443.txt),
  [BIOS](logs/fase-3-permissoes-final-nas-20261115-150443-qemu-kvm-bios.log) e
  [UEFI](logs/fase-3-permissoes-final-nas-20261115-150443-qemu-kvm-uefi.log).

Próximos gates: armazenar contas de forma protegida, implementar operações
VFS com credenciais para leitura/escrita/criação/remoção, adicionar ACLs quando
necessárias e conectar autorização à syscall e ao processo isolado em ring 3.

## Identidade por processo na ABI Linux (2026-11-16)

O dispatcher Linux implementa `getuid(102)`, `getgid(104)`, `geteuid(107)`,
`getegid(108)` e `getgroups(115)`. O kernel preenche o contexto da chamada com
UID/GID e grupos suplementares do processo escalonado
(`ProcessSecurityContext`), em vez de uma identidade global. Como o modelo
ainda não diferencia UID/GID real de efetivo, cada par retorna o mesmo valor.
`getgroups` valida capacidade e copia apenas grupos suplementares para user.
Os testes host cobrem credenciais não-root, grupos e cópia para user; o probe
CPL3 do kernel ainda não chama essas syscalls.

Validação: 120 testes host local e NAS, build `x86_64-unknown-none` e boots
BIOS/UEFI local e NAS/KVM passaram. Evidências: [host local](logs/fase3-5-credenciais-syscall-host-20261116-015050.txt),
[host NAS](logs/fase3-5-credenciais-syscall-nas-20261116-015111.txt),
[BIOS local](logs/fase3-5-credenciais-syscall-qemu-bios-20261116-015050.log),
[UEFI local](logs/fase3-5-credenciais-syscall-qemu-uefi-20261116-015050.log),
[BIOS NAS](logs/fase3-5-credenciais-syscall-nas-20261116-015111-qemu-kvm-bios.log)
e [UEFI NAS](logs/fase3-5-credenciais-syscall-nas-20261116-015111-qemu-kvm-uefi.log).

## Primeira fundação modular

- `src/vfs.rs` adiciona árvore de diretórios fixa sem heap, permissões Unix
  básicas de leitura/escrita/execução, verificação de proprietário e um layout
  em `/programas`, `/configuracoes`, `/usuarios`, `/sistema`, `/dados` e
  `/temporarios`.
- `src/userland/login.rs` define prompt pt_BR e autenticação via verificador
  injetável. Nenhuma senha é armazenada pelo gerenciador; falta backend ligado
  ao armazenamento protegido.
- `src/userland/apps.rs` cadastra programas e exige permissão de execução no
  VFS antes de delegar ao carregador. O carregador ELF e o scheduler ainda não
  existem; portanto, não há execução de binários nesta etapa.
- `src/memory.rs` passa a incluir alocador de frames por bitmap fornecida pelo
  chamador, com detecção de alinhamento, capacidade e double-free. Ainda não
  consome diretamente o mapa Limine nem está ligado à paginação do kernel.
- O login modular apresenta prompts pt_BR e usa verificador injetável. O
  catálogo de apps checa execução no VFS e delega a um `ProgramLoader`; ambos
  são contratos host, sem login de boot ou execução ELF real.
- `cargo test --all-targets`: 29 testes passaram localmente. Boot/smoke normal
  BIOS no QEMU TCG passou, confirmando que o kernel de diagnóstico segue
  inicializando; log: [`logs/fase-3-server-qemu-local.log`](logs/fase-3-server-qemu-local.log).
- `scripts/test-local-iso.sh` passou testes host, build ISO e smoke BIOS/UEFI em
  QEMU TCG. Logs:
  [`logs/fase-3-ciclo-host-20261115-131039.txt`](logs/fase-3-ciclo-host-20261115-131039.txt),
  [`logs/fase-3-ciclo-qemu-bios-20261115-131039.log`](logs/fase-3-ciclo-qemu-bios-20261115-131039.log) e
  [`logs/fase-3-ciclo-qemu-uefi-20261115-131039.log`](logs/fase-3-ciclo-qemu-uefi-20261115-131039.log).
- QEMU e testes locais não validam hardware real do servidor. O acesso ao NAS
  retornou `No route to host`; validação remota fica pendente conforme
  orientação do usuário.

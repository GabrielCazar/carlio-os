# Reprodução dos testes TCP no NAS

O executor usa QEMU/KVM no NAS Manjaro. Precisa de acesso a `/dev/kvm`,
QEMU, OVMF, Python, binutils, util-linux e e2fsprogs no usuário SSH.
O perfil TCP abre uma porta efêmera apenas no loopback de cada host de teste.

O disco auxiliar do NAS fica em
`/home/gabriel/.local/share/carlioos-test/disks/test-20g.qcow2`.
É uma imagem QCOW2 esparsa com capacidade virtual de 21.474.836.480 bytes.
O runner TCP anexa a imagem em snapshot descartável. O ELF CPL3 fica em
outro disco, um ext4 descartável criado pelo próprio teste. O perfil padrão
de armazenamento usa o disco próprio do runner por padrão. Em outro NAS,
crie a imagem apenas se o caminho escolhido estiver livre.

```bash
umask 077
dir="$HOME/.local/share/carlioos-test/disks"
mkdir -p "$dir"
if [ ! -e "$dir/test-20g.qcow2" ]; then
  qemu-img create -f qcow2 "$dir/test-20g.qcow2" 20G
fi
qemu-img info "$dir/test-20g.qcow2"
```

```bash
CARLIO_DISK_EXEC_TEST=1 ./scripts/build-iso.sh

CARLIO_TEST_SSH_CONFIG=/home/gabriel/.ssh/config \
CARLIO_TEST_PHASE=bloco-4-tcp-bloqueante \
CARLIO_TEST_QEMU_ONLY=1 \
CARLIO_TEST_TCP_BLOCKING=1 \
CARLIO_TEST_VM_DISK=/home/gabriel/.local/share/carlioos-test/disks/test-20g.qcow2 \
./scripts/test-nas.sh
```

O modo `CARLIO_TEST_TCP_BLOCKING=1` usa
`build/carlio-os-disk-exec-test.iso` por padrão. A imagem padrão foi
revalidada separadamente. As duas execuções no NAS usaram o disco de
20 GiB em snapshot. [Boot BIOS](../reports/logs/beta-regressao-nas-20261126-182509-qemu-kvm-bios.log) ·
[Boot UEFI](../reports/logs/beta-regressao-nas-20261126-182509-qemu-kvm-uefi.log) ·
[TCP BIOS](../reports/logs/beta-tcp-bloqueante-nas-20261126-182547-qemu-kvm-bios.log) ·
[TCP UEFI](../reports/logs/beta-tcp-bloqueante-nas-20261126-182547-qemu-kvm-uefi.log)

Com `CARLIO_TEST_VM_DISK` omitido, o perfil TCP cria um QCOW2 descartável
de 20 GiB no diretório temporário do runner. Os logs BIOS e UEFI são
copiados para `reports/logs/` antes de remover o diretório remoto da execução.
`CARLIO_TEST_OVMF` e `CARLIO_TEST_OVMF_VARS` permitem escolher o par de
firmware e variáveis UEFI.

A prova mede uma espera de 50 ms com o relógio do convidado, executa
`accept4` via `int 0x80`, atrasa a conexão e a entrega dos dados, verifica
despertar do `poll` por tráfego, executa duas escritas seguidas e exige FIN
e saída CPL3 com status zero. A prova de scheduler em Cargo também verifica
que outra CPU não toma o frame de uma tarefa recém-acordada.

## Diretório de execução no NAS

O executor cria cada execução em um subdiretório privado de
`~/.local/share/carlioos-test/runs` no NAS. Ele usa esse espaço para o
código-fonte, a ISO, o Cargo e seus arquivos temporários. Depois de copiar
os logs para `reports/logs`, remove o subdiretório da execução. Um disco
de teste configurado pelo operador fica fora desse diretório.
O perfil de serviços foi executado a partir desse diretório em KVM, com
o disco de 20 GiB em snapshot.
[BIOS](../reports/logs/nas-diretorio-dedicado-nas-20261126-105322-qemu-kvm-bios.log) · [UEFI](../reports/logs/nas-diretorio-dedicado-nas-20261126-105322-qemu-kvm-uefi.log)

## Resultados de 26/11/2026

O teste TCP passou com TCG local e com KVM no NAS, em BIOS e UEFI.
Ambos os boots no NAS registraram a saída CPL3 com status zero. O host
conferiu os bytes da resposta e o FIN.

- [Relatório do executor e metadados do QCOW2](../reports/logs/bloco-4-tcp-bloqueante-revalidacao-nas-20261126-095236.txt)
- [KVM BIOS](../reports/logs/bloco-4-tcp-bloqueante-revalidacao-nas-20261126-095236-qemu-kvm-bios.log)
- [KVM UEFI](../reports/logs/bloco-4-tcp-bloqueante-revalidacao-nas-20261126-095236-qemu-kvm-uefi.log)

O smoke com a imagem como disco principal também passou em BIOS e UEFI.
O driver AHCI identificou 41.943.040 setores de 512 bytes em ambos.
Na primeira tentativa, os APs entraram na barreira do reaper com
interrupções desabilitadas e o shootdown de TLB expirou. A revalidação usa
o caminho ocioso corrigido, em que os APs continuam recebendo o IPI
durante a barreira.

- [Disco principal no KVM BIOS](../reports/logs/disco-20g-revalidacao-nas-20261126-095205-qemu-kvm-bios.log)
- [Disco principal no KVM UEFI](../reports/logs/disco-20g-revalidacao-nas-20261126-095205-qemu-kvm-uefi.log)

## Supervisão de processos TCP

No console autenticado, `servico iniciar /CAMINHO [ARGUMENTOS]` carrega um ELF
com o UID, GID e grupos suplementares da sessão, retorna o PID e libera o
console. `servicos` lista os processos do usuário, `servico parar PID` encerra
um processo desse usuário e `servico reiniciar PID` o relança. Cada processo
usa uma CPU secundária, portanto a VM precisa de pelo
menos duas vCPUs e tem capacidade de um processo supervisionado por vCPU
secundária. O processo continua após o logout do operador.

O teste prepara uma imagem RAW descartável com duas contas e um servidor ELF
de eco TCP. Ele verifica UID, GID, grupos e isolamento da serial em
CPL3. O cliente Python abre uma conexão, mantém a sessão ativa enquanto o
supervisor para outro processo ocupado em CPL3, testa a recusa de parada para
outro usuário e confirma o encerramento da conexão antiga. Depois conecta na
mesma porta com outro PID. O encaminhamento de porta fica no loopback do host.

```sh
TMPDIR="$PWD/target" ./scripts/build-iso.sh
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test-service-qemu.py \
  build/carlio-os.iso target/servico-bios.log
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test-service-qemu.py \
  build/carlio-os.iso target/servico-uefi.log \
  --firmware /usr/share/edk2/x64/OVMF_CODE.4m.fd \
  --vars /usr/share/edk2/x64/OVMF_VARS.4m.fd
```

No NAS, o mesmo perfil roda em KVM com o QCOW2 de 20 GiB como disco
secundário em snapshot. A imagem de contas permanece descartável.

```sh
CARLIO_TEST_SSH_CONFIG="$HOME/.ssh/config" \
CARLIO_TEST_PHASE=bloco-4-supervisor \
CARLIO_TEST_SERVICES=1 \
CARLIO_TEST_VM_DISK=/home/gabriel/.local/share/carlioos-test/disks/test-20g.qcow2 \
  ./scripts/test-nas.sh
```

A primeira execução em KVM mostrou que a saída do ELF e a confirmação do
console podiam misturar bytes na UART. O supervisor passou a iniciar serviços
com os descritores seriais fechados. A revalidação passou nos dois modos de
boot com o disco de 20 GiB. [BIOS](../reports/logs/bloco-4-supervisor-nas-20261126-100800-qemu-kvm-bios.log) · [UEFI](../reports/logs/bloco-4-supervisor-nas-20261126-100800-qemu-kvm-uefi.log)

## Revisão do TCP

O driver e1000 preserva a fila de recepção quando a tabela de sockets está
ocupada. A pilha TCP mantém na fila de retransmissão apenas os bytes ainda
pendentes após um ACK parcial e evita atribuir uma porta efêmera ainda em
encerramento. Os testes de regressão cobrem ACK parcial com retorno a zero
do número de sequência e reconexão antes da confirmação do FIN.

Após as correções, os 234 testes da biblioteca passaram. O teste de reinício
do serviço de eco TCP passou em KVM com o disco de 20 GiB em snapshot.
[BIOS](../reports/logs/bloco-4-auditoria-tcp-nas-20261126-102445-qemu-kvm-bios.log) · [UEFI](../reports/logs/bloco-4-auditoria-tcp-nas-20261126-102445-qemu-kvm-uefi.log)

## Aceite de SSH/SFTP e SMB

A prova de compatibilidade do OpenSSH cria um disco ext4 descartável com
o `sshd` do host, seu interpretador ELF e bibliotecas. Dentro do convidado,
um executável chama `execve("/usr/bin/sshd", [..., "-V"], ...)` e o teste
exige a versão na serial e status zero. O disco contém apenas o executável,
suas dependências e o lançador de diagnóstico.

```sh
CARLIO_DISK_EXEC_TEST=1 TMPDIR="$PWD/target" ./scripts/build-iso.sh
TMPDIR="$PWD/target" ./scripts/test-openssh-qemu.sh \
  build/carlio-os-disk-exec-test.iso target/openssh.log --accel tcg --timeout 90
```

O `sshd` abre `/dev/null` durante a inicialização. O kernel fornece o
dispositivo de caracteres pela ABI, com EOF em leituras e descarte de
escritas. O teste de versão cobre a carga e inicialização do binário.
Execução como daemon, autenticação e transferência SFTP continuam pendentes.
No NAS, o OpenSSH 10.6p2 imprimiu a versão e saiu com status zero nos
dois modos de boot. O teste conferiu também a liberação dos frames de
processos, contabilizando separadamente os frames que o heap do kernel
reteve para futuras alocações.
[BIOS](../reports/logs/bloco-4-openssh-version-nas-recheck-bios.log) · [UEFI](../reports/logs/bloco-4-openssh-version-nas-recheck-uefi.log)

O perfil `CARLIO_OPENSSH_TEST=config` prepara uma chave de host Ed25519
descartável e usa `sshd -T` para validar a configuração e a chave dentro
do convidado. O teste exige o caminho da chave, a porta 2222 e status zero.
Durante a inicialização, a libc usa `FUTEX_WAKE`. A ABI aceita a operação
com ou sem a flag privada e retorna zero quando não há tarefas em espera.
`FUTEX_WAIT` ainda está pendente.
As capturas em BIOS e UEFI registram a porta, a chave e o status zero.
Repetições também encontraram travamentos intermitentes no caminho de
memória do boot. O perfil ainda precisa de estabilização para servir
como critério de aceite.
[BIOS](../reports/logs/bloco-4-openssh-config-nas-bios.log) · [UEFI](../reports/logs/bloco-4-openssh-config-nas-uefi.log)

Para diagnosticar a próxima etapa, execute o mesmo script com
`CARLIO_OPENSSH_TEST=listen`. O perfil executa `sshd -D -e -f /etc/ssh/probe_config`
e exige que o próprio OpenSSH confirme o listener IPv4 na porta 2222.
O teste encerra a VM ao registrar esse evento. O perfil `banner` também
encaminha uma porta efêmera de loopback do host para o convidado e exige
uma identificação `SSH-2.0-OpenSSH_…` recebida por um cliente TCP.
A autenticação e o SFTP têm critérios de aceite próprios.

Durante o diagnóstico SMP, `info registers -a` e `info lapic` do monitor
QEMU mostraram o BSP em uma leitura AHCI com interrupções mascaradas e o
vetor 241 (`0xf1`) pendente. Um AP aguardava três confirmações e tinha
recebido duas. O polling de comandos AHCI agora permite interrupções
durante as leituras MMIO e restaura o estado anterior ao sair, mantendo o
lock do DMA e o limite de polling. A captura intermediária também confirmou a abertura
do listener em UEFI.
[Listener UEFI](../reports/logs/bloco-4-openssh-listen-nas-uefi.log) ·
[Timeout de ACK](../reports/logs/bloco-4-openssh-irq-stack-banner-nas-uefi.log)

Na prova local com TCG, o cliente atingiu o `ppoll`, o `sshd` aceitou a
conexão e a libc criou um filho via `clone`. A captura registrou a falha
anterior de `dup2` ao redirecionar o socket para stdin. O kernel passou a
oferecer aliases de sockets TCP em stdin, stdout e stderr e de
`socketpair` em números baixos. A alocação de sockets IPv4 pula os números
ocupados por esses aliases. O teste TCP local passou com `clone`, `wait4`,
herança, redirecionamento e reinício pelo supervisor. No OpenSSH, a
execução chegou à recarga do `sshd-session`, cujo binário do Arch
ultrapassava o limite anterior de 512 KiB. O buffer de ELF principal
agora aceita até 2 MiB, e a fixture inclui as bibliotecas dos auxiliares.
[Serviço TCP em TCG](../reports/logs/bloco-4-descritores-compartilhados-tcg.log) ·
[Recarga do sshd-session](../reports/logs/bloco-4-openssh-banner-config-fd-tcg.log)

A prova de supervisão com `clone` e aliases TCP de entrada e saída padrão
passou em BIOS e UEFI no KVM do NAS com o QCOW2 de 20 GiB em snapshot.
[BIOS](../reports/logs/bloco-4-dup-tcp-clone-nas-20261126-174502-qemu-kvm-bios.log) ·
[UEFI](../reports/logs/bloco-4-dup-tcp-clone-nas-20261126-174502-qemu-kvm-uefi.log)

A prova em TCG confirmou que a versão de 2 MiB executa
`/usr/lib/ssh/sshd-session`. O carregador dessa distribuição falhou ao
mapear `libbrotlienc.so.1`. O limite de regiões por processo passou de 64
para 128 para acomodar a quantidade de segmentos das bibliotecas. A cota
de folhas continua em 4096. Em TCG, a rodada com 128 regiões superou
o erro de mapeamento e chegou a `dup2` de `/dev/null` sobre o descritor
do canal de configuração. O kernel passou a suportar essa duplicação.
Na rodada UEFI/KVM anterior à alteração de regiões, o listener abriu,
mas o cliente ainda não recebeu o banner.
[TCG com 64 regiões](../reports/logs/bloco-4-openssh-session-banner-completo-tcg.log) ·
[TCG com 128 regiões](../reports/logs/bloco-4-openssh-session-vmas-tcg.log) ·
[UEFI/KVM](../reports/logs/bloco-4-openssh-session-banner-nas-uefi.log)

A duplicação de socket TCP via `dup` e `F_DUPFD` também respeita
descritores ocupados por arquivos, IPC e console. A captura que preserva
o prazo padrão do OpenSSH chegou ao `setsockopt` do socket e à chamada
`setitimer`, que ainda falhou. A fixture `banner`, acessível somente por
loopback do host, usa `LoginGraceTime 0` para medir a identificação do
protocolo dentro do prazo do executor. O temporizador do servidor, o
tratamento de sinais e a autenticação continuam sendo critérios de aceite.
[Serviço TCP local](../reports/logs/bloco-4-servico-dup-socket-tcg.log) ·
[OpenSSH com o prazo padrão](../reports/logs/bloco-4-openssh-socket-dup-tcg.log) · [UEFI/KVM](../reports/logs/bloco-4-openssh-socket-dup-nas-uefi.log)

O perfil de loopback chegou à separação de privilégios pré-autenticação
em TCG. O filho recusou continuar quando `setrlimit(RLIMIT_FSIZE, {0,0})`
retornou `ENOSYS`. Em seguida, a limpeza registrou que `kill` também
está ausente. Em UEFI/KVM, a rodada de loopback abriu o listener e atingiu
a preparação do socket de sessão, mas expirou sem banner.
O aceite requer limites e sinais
efetivos no kernel e a preservação da separação de privilégios.
[Pré-autenticação em TCG](../reports/logs/bloco-4-openssh-banner-loopback-tcg.log) ·
[Loopback em UEFI/KVM](../reports/logs/bloco-4-openssh-banner-loopback-nas-uefi.log)

A ABI fornece `getpeername` para sockets TCP conectados. A consulta usa os
descritores do processo chamador. O teste do serviço de eco compara o endereço
e a porta devolvidos pela syscall com os obtidos em `accept`.
O teste passou nos dois modos de boot no KVM do NAS.
[BIOS](../reports/logs/bloco-4-peer-name-nas-20261126-103241-qemu-kvm-bios.log) · [UEFI](../reports/logs/bloco-4-peer-name-nas-20261126-103241-qemu-kvm-uefi.log)

Use `servico reiniciar PID` no console autenticado para testar o reinício do
ELF supervisionado. O comando usa os argumentos e as credenciais originais,
libera os recursos do processo anterior e devolve o novo PID. Se a nova carga
falhar, o comando informa o erro e o serviço fica parado. O teste de eco
verifica a recusa para outro usuário, o fechamento da conexão anterior e a
reabertura da mesma porta com o novo PID.

O reinício direto passou em [BIOS](../reports/logs/bloco-4-reinicio-nas-20261126-101529-qemu-kvm-bios.log)
e [UEFI](../reports/logs/bloco-4-reinicio-nas-20261126-101529-qemu-kvm-uefi.log)
no KVM do NAS com o disco de 20 GiB em snapshot.

Faltam os servidores de protocolo e a ligação da identidade autenticada de
cada cliente às autorizações do VFS e à execução de processos. A ABI já
permite que uma tarefa root de thread única ajuste GID e até oito grupos
suplementares e depois reduza seu UID de forma permanente por
`setgid`, `setgroups` e `setuid`. O modelo mantém um único UID e um único
GID, compartilhados pelos getters real e efetivo. Chamadas posteriores
usam o contexto atualizado, inclusive para permissões do VFS. O supervisor
usa a identidade do operador local para iniciar o ELF e conserva essa
identidade para autorizar e executar um futuro reinício. O fluxo normal do
OpenSSH usa processos de sessão com separação de privilégios, e o Samba cria
filhos por conexão mesmo no modo foreground. O porte desses servidores requer
suporte a criação e supervisão de processos descendentes, IPC e credenciais de
sessão na ABI. Consulte o [OpenSSH Portable](https://github.com/openssh/openssh-portable/blob/master/README.privsep)
e o [manual do smbd](https://www.samba.org/samba/docs/current/man-html/smbd.8.html).

O teste unitário do escalonador confirma a troca de GID e grupos, a redução
de UID e a recusa de retorno a root. O teste do serviço TCP em CPL3 confirma
que as três syscalls recusam as alterações solicitadas por uma conta comum.
[BIOS](../reports/logs/bloco-4-credenciais-nas-20261126-103800-qemu-kvm-bios.log) · [UEFI](../reports/logs/bloco-4-credenciais-nas-20261126-103800-qemu-kvm-uefi.log)

Antes de abrir o listener, o serviço de teste cria um pipe e chama `fork`.
O filho confere `getppid`, escreve pelo pipe herdado e sai com status 37.
O pai confirma o PID e o status via `wait4`, lê o byte e verifica EOF.
[BIOS](../reports/logs/bloco-4-fork-pipe-nas-20261126-110748-qemu-kvm-bios.log) · [UEFI](../reports/logs/bloco-4-fork-pipe-nas-20261126-110748-qemu-kvm-uefi.log)

O teste de herança de arquivos abre um ELF antes de `fork`. O filho lê o
primeiro byte e o pai lê o segundo pelo descritor original após `wait4`,
confirmando que ambos usam a mesma posição de leitura.
[BIOS](../reports/logs/bloco-4-fork-file-nas-20261126-111103-qemu-kvm-bios.log) · [UEFI](../reports/logs/bloco-4-fork-file-nas-20261126-111103-qemu-kvm-uefi.log)

O teste de herança de TCP cria um filho depois de `accept`. O filho fecha
sua referência ao listener e responde ao cliente pela conexão herdada.
Depois de recolher o filho, o pai continua atendendo pela conexão original.
[BIOS](../reports/logs/bloco-4-fork-socket-nas-20261126-111354-qemu-kvm-bios.log) · [UEFI](../reports/logs/bloco-4-fork-socket-nas-20261126-111354-qemu-kvm-uefi.log)

`socketpair(AF_UNIX, SOCK_STREAM, 0)` cria um canal bidirecional anônimo,
com 4 KiB de buffer em cada direção. As pontas aceitam `read`, `write`,
`sendto` e `recvfrom` sem endereço, além de `poll`, duplicação e herança
em `fork`. É possível solicitar `SOCK_NONBLOCK` e `SOCK_CLOEXEC`.
Nos pares locais, `shutdown` pode desativar a recepção, o envio ou ambos.
Após o encerramento de uma direção, o receptor ainda pode ler os bytes
enfileirados antes de obter EOF. Passagem de descritores por `SCM_RIGHTS`
permanece pendente.
O teste de `SHUT_WR` confirmou EOF no filho antes de o pai fechar sua ponta
e verificou que o filho ainda conseguia enviar a resposta.
[BIOS](../reports/logs/bloco-4-socket-shutdown-nas-20261126-111900-qemu-kvm-bios.log) · [UEFI](../reports/logs/bloco-4-socket-shutdown-nas-20261126-111900-qemu-kvm-uefi.log)

O TCP também implementa `shutdown` em conexões estabelecidas. Com
`SHUT_WR`, o kernel transmite os dados pendentes antes de emitir FIN e
continua recebendo pela conexão. `SHUT_RD` permite drenar os dados já
enfileirados e descarta os dados posteriores.
O cliente do teste recebeu o byte pendente e EOF, enviou um novo byte
pela mesma conexão e verificou que o serviço continuou aceitando conexões.
[BIOS](../reports/logs/bloco-4-tcp-shutdown-nas-20261126-112144-qemu-kvm-bios.log) · [UEFI](../reports/logs/bloco-4-tcp-shutdown-nas-20261126-112144-qemu-kvm-uefi.log)
O teste em CPL3 troca bytes entre pai e filho nos dois sentidos e verifica
EOF depois que o filho encerra.
[BIOS](../reports/logs/bloco-4-socketpair-nas-20261126-111632-qemu-kvm-bios.log) · [UEFI](../reports/logs/bloco-4-socketpair-nas-20261126-111632-qemu-kvm-uefi.log)

A aceitação de SSH/SFTP exige autenticação, identidade do host, execução com
a identidade autenticada e transferência de arquivos com permissão por usuário.
A aceitação de SMB exige negociação SMB2/3, autenticação, compartilhamentos
com permissão por usuário e operações de arquivos. Em ambos, um cliente
independente deve reconectar depois de parar e iniciar o serviço, e uma sessão
antiga deve ser encerrada. O teste de eco valida somente o ciclo de vida TCP.

A ABI disponibiliza `pipe` e `pipe2` com `O_NONBLOCK` e `O_CLOEXEC`. Cada
pipe tem um buffer de 4 KiB, com espera de `read` e `write` ligada ao
escalonador e estado visível por `poll`. `fork` herda os descritores de pipe
com os mesmos números. Cada processo fecha suas próprias referências.
A escrita com o
leitor fechado retorna `EPIPE`; a entrega de SIGPIPE permanece pendente.
Nos pipes e sockets IPv4, `fcntl` implementa `F_GETFD`, `F_SETFD`,
`F_GETFL` e `F_SETFL` para consultar o modo de acesso e controlar
`FD_CLOEXEC` e `O_NONBLOCK`. Cada ponta de um pipe mantém suas próprias
flags. `dup`, `F_DUPFD` e `F_DUPFD_CLOEXEC` duplicam descritores de pipe
na faixa reservada a pipes. Os aliases compartilham `O_NONBLOCK` e mantêm
`FD_CLOEXEC` individual. A ponta permanece aberta enquanto houver um alias.
`dup2` e `dup3` também podem direcionar uma ponta de pipe para entrada,
saída ou erro padrão, ou para um descritor na faixa reservada a pipes.
A duplicação de outros tipos de descritor continua pendente.
Em CPL3, o teste redirecionou a saída padrão com `dup3`, confirmou que
`dup2(fd, fd)` preserva `FD_CLOEXEC` e transferiu um byte pelo pipe.
[BIOS](../reports/logs/bloco-4-dup-stdio-nas-20261126-110038-qemu-kvm-bios.log) · [UEFI](../reports/logs/bloco-4-dup-stdio-nas-20261126-110038-qemu-kvm-uefi.log)
O teste em CPL3 usou o alias de escrita depois de fechar o descritor original.
[BIOS](../reports/logs/bloco-4-dup-pipe-nas-20261126-105818-qemu-kvm-bios.log) · [UEFI](../reports/logs/bloco-4-dup-pipe-nas-20261126-105818-qemu-kvm-uefi.log)
O teste em CPL3 verificou os modos de acesso e as flags de pipe e socket.
[BIOS](../reports/logs/bloco-4-fcntl-nas-20261126-105607-qemu-kvm-bios.log) · [UEFI](../reports/logs/bloco-4-fcntl-nas-20261126-105607-qemu-kvm-uefi.log)
O serviço de teste verifica em CPL3 a leitura não bloqueante de um pipe
vazio, a transferência de um byte e EOF após fechar o descritor de escrita.
[BIOS](../reports/logs/bloco-4-pipe-nas-20261126-104927-qemu-kvm-bios.log) · [UEFI](../reports/logs/bloco-4-pipe-nas-20261126-104927-qemu-kvm-uefi.log)

Com os servidores configurados no convidado, execute o teste básico a partir
do host, usando um diretório inicial SFTP e um compartilhamento SMB graváveis
e reservados para testes.

```bash
python3 scripts/test-file-services.py \
  --ssh-target operador@carlio-teste --ssh-port 2222 \
  --known-hosts ./known_hosts.carlio --uid 1000 \
  --smb-share //carlio-teste/teste --smb-port 1445 \
  --smb-auth-file ./credenciais-smb
```

Prepare a chave de host confiável no arquivo `known_hosts.carlio`, uma chave
de cliente para o operador e um arquivo de autenticação do `smbclient` com
permissão 0600. O teste consulta o UID via `ssh` e envia, baixa, compara e
remove arquivos temporários via `sftp` e `smbclient`. Execute antes e depois
de reiniciar os servidores. O encerramento de sessões antigas e a recusa
de acesso entre contas ainda precisam ser testados separadamente.

### Recuperação do overlay e isolamento SMP

O teste de recuperação usa apenas uma imagem RAW descartável. Primeiro
confirma a persistência de conteúdo e permissões entre boots. Depois simula
um cabeçalho interrompido e exige que o kernel recuse a montagem do snapshot
anterior, bloqueie as contas e rejeite uma mutação. O SHA-256 da imagem deve
permanecer igual durante o boot degradado. Um snapshot anterior pode conter
permissões ou credenciais revogadas. Preserve uma cópia da imagem para análise
offline antes de qualquer intervenção.

```bash
cargo test --offline --lib vfs_store
CARLIO_STORAGE_TEST=1 ./scripts/build-iso.sh
python3 scripts/test-persistence-qemu.py --iso build/carlio-os-storage-test.iso \
  --vfs --log-dir target/overlay-bios
python3 scripts/test-persistence-qemu.py --iso build/carlio-os-storage-test.iso \
  --vfs --firmware /usr/share/edk2/x64/OVMF_CODE.4m.fd --log-dir target/overlay-uefi
```

Para a prova SMP sustentada, gere a ISO com
`CARLIO_SMP_STRESS_TEST=1 CARLIO_STACK_WATERMARK=1 ./scripts/build-iso.sh`.
Execute `scripts/qemu-smoke.py` com `--cpus 4 --smp-cycles 8192
--expect-runtime-stress --accel kvm` em BIOS e UEFI. O smoke verifica os ciclos
por AP, a sobreposição do trabalho, o retorno a idle, o isolamento de dois
page faults e as amostras de uso de pilha. As amostras medem uso de pilha.
A proteção por páginas de guarda exige um teste próprio.

Registre a revisão de código e o SHA-256 de cada artefato. Os perfis com
instrumentação servem para diagnóstico. A homologação deve testar também a ISO
de distribuição, gerada da mesma revisão aprovada nos critérios da beta.

Para repetir a prova de banner do OpenSSH com o prazo padrão do daemon,
use `CARLIO_OPENSSH_TEST=banner CARLIO_OPENSSH_BANNER_GRACE=default` ao
executar `scripts/test-openssh-qemu.sh` com a ISO `disk-exec-test`. O
perfil compara o banner pelo loopback e mantém o prazo externo do
QEMU. Use o log serial para identificar a primeira falha da ABI.

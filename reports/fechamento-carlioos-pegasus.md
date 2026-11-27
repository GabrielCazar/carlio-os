# Critérios pendentes do CarlioOS e Pegasus DS

| Bloco | Evidência disponível | Critério ainda aberto |
| --- | --- | --- |
| 0 · Contrato beta | Escopo NAS x86-64 e matriz de hardware documentados | Associar evidências de aceite a uma revisão imutável |
| 1 · Boot e hardware | BIOS/UEFI em VM, inventário AHCI e Realtek no NAS | Boot físico, AHCI Intel com mídia descartável e RTL8111 com TX/RX |
| 2 · Isolamento | Guardas e carga SMP em BIOS e UEFI, com 8.192 ciclos por AP de teste e dois faults CPL3 isolados | Homologação da revisão de distribuição |
| 3 · Bloco e recuperação | Testes de recuperação do overlay em VM e bloqueio de contas em estado degradado | Escrita e recuperação AHCI física e validação do estado persistente do produto |
| 4 · VFS, contas e rede | Contas, autorização e provas de sockets; listener OpenSSH em VM | Identidade de objetos abertos, OpenSSH/SFTP autenticados, SMB e rede física |
| 5 · ABI Linux | ELF dinâmico e diagnóstico Pegasus em CPL3; provas de arranque de Samba e pacman | ABI suficiente para serviços reais e transações assinadas no userland Arch |
| 6 · Interface e agentes | Console headless | Administração headless integrada, interfaces de desktop e agentes conforme escopo |
| 7 · Instalador | Imagem virtual com SHA-256 verificado e boot BIOS/UEFI | Instalador físico, preservação de mídia, atualização e rollback da revisão aprovada |
| 8 · Windows | PE32+ validado, Registry convidado, broker e supervisor QEMU de referência | Gerenciador nativo de VMs, canal kernel–guest, imports Win32 e execução de PE em CPL3 |

O Pegasus DS já executa operações de diretório em memória no CarlioOS com
identidades sintéticas. Em BIOS e UEFI, a prova confirmou recusa anônima,
alterações auditadas, bloqueio de alteração se a auditoria falhar e saída zero.
No host, os testes do workspace, a formatação e o Clippy com warnings como
erros passaram. O contrato e o roadmap de fases do Pegasus DS permanecem
como fonte do aceite do produto.

| Fases Pegasus DS | Critério ainda aberto |
| --- | --- |
| 0–1 · Fundação, identidades e dados | Checkpoint de diagnóstico validado no host. Faltam backend transacional, gestão de chaves e homologação da recuperação no volume de destino |
| 2–3 · LDAP, autenticação e descoberta | LDAP em rede com TLS 1.3, Kerberos/GSSAPI, DNS/SRV e sincronização de tempo |
| 4–5 · Administração e alta disponibilidade | Políticas assinadas, rollback, operação offline, replicação mTLS e teste de desastre |
| 6 · Integração CarlioOS | Backend de dircache em userspace e teste do contrato com uma revisão fixada antes de avaliar uma ponte adicional |
| 7 · Lançamento | Interoperabilidade, fuzzing contínuo, artefatos assinados, recuperação e revisão externa |

A rodada conjunta depende de uma janela de boot no NAS e de um disco
descartável para a prova destrutiva. O NAS em operação tem dois discos montados
e um dispositivo que reporta 0 B. Preserve a evidência de cada perfil
diagnóstico e homologue a ISO de distribuição a partir da revisão aprovada.

## Provas de arranque da ABI

A primeira execução do Samba parou no nome
`libsamba3-util-private-samba.so`. O VFS passou a consultar o backend
montado para esse tipo de caminho após verificar as permissões de
travessia dos diretórios do overlay. A regressão verifica o caso normal e
a recusa sob um diretório protegido.

Após a correção, a ISO de diagnóstico confirmou dois limites diferentes.
O Samba esgotou a tabela de regiões virtuais ao preparar a busca de
símbolos. O pacman esgotou a capacidade de páginas ao mapear um segmento
de `libssl.so.3`. Ambos encerraram com status 127.
As provas são reproduzíveis com o procedimento na
[Matriz de pacotes](../docs/MATRIZ-PACOTES.md).

## Base do kernel validada em 2026-11-27

- **Isolamento.** As páginas de guarda passaram em BIOS e UEFI no QEMU
  local e no KVM do NAS. O teste de ultrapassagem da pilha do BSP
  confirmou o diagnóstico por IST. [Carga SMP e logs](fase-2-memoria-execucao.md)
- **Autorização do VFS.** A prova CPL3 exige `EACCES` para `access`,
  `openat`, `execve` e `newfstatat` sob diretório ext4 protegido.
  Verifica também `openat` com criação, `mkdir`, `chmod`, `unlink`
  e `rmdir` sob diretório protegido do overlay, bem como leitura permitida
  e escrita, truncamento e execução negados em arquivo somente leitura.
  Criação com ancestral ausente retorna `ENOENT` e mantém o ancestral
  ausente. Ao projetar um ancestral do volume, o overlay preserva UID,
  GID e modo. [BIOS](logs/guardas-20261127-cpl3-vfs-bios.log) ·
  [UEFI](logs/guardas-20261127-cpl3-vfs-uefi.log)
- **Pegasus DS.** O diagnóstico de diretório em memória encerrou com
  status zero em CPL3 nas duas firmwares, com o checkout local baseado em
  `51d72ef` e alterações de trabalho. Persistência transacional,
  identidade de arquivos abertos, LDAP em rede e autenticação do serviço
  seguem como critérios de aceite da integração. [BIOS](logs/guardas-20261127-pegasus-ds-bios.log) ·
  [UEFI](logs/guardas-20261127-pegasus-ds-uefi.log)
- **Pegasus FS.** `BlockDevice` aceita escrita multissetor com verificação
  do intervalo inteiro antes de qualquer I/O. `BlockRegion` limita a escrita
  à partição. Falhas do dispositivo podem deixar um prefixo escrito e
  a durabilidade depende do `flush`. Os 11 testes do workspace do
  Pegasus FS passaram no checkout local baseado em `e6dd3c8` com alterações
  de trabalho. O overlay do CarlIO recuperou dados e permissões após
  reboot. Em BIOS e UEFI, detectou também o cabeçalho interrompido e
  bloqueou gravações no estado degradado, com SHA-256 inalterado.
  O porte para o VFS do kernel depende do aceite do protótipo
  em userspace e da prova de consistência do formato.
  [Testes do Pegasus FS](logs/guardas-20261127-pegasus-fs-host.txt) ·
  [Recuperação BIOS](logs/guardas-20261127-recovery/fase3-vfs-bios-recuperar.log) ·
  [Recuperação UEFI](logs/guardas-20261127-recovery/fase3-vfs-uefi-recuperar.log)

O teste do host do CarlIO OS passou com 268 testes e um ignorado.
`cargo fmt --check` encontrou diferenças no workspace. O Clippy com
warnings como erros parou em duas lints de `src/pipe.rs`.
[Testes](logs/guardas-20261127-host.txt) ·
[Formatação](logs/guardas-20261127-fmt.txt) · [Clippy](logs/guardas-20261127-clippy.txt)

## Dircache supervisionado

A build opt-in vincula o backend assinado a um UID de serviço e à tabela
`/etc/pegasusds/identities.tsv` no lançamento pelo console. O backend
preserva suas credenciais POSIX e recebe somente a autorização de registro
no broker. A parada do serviço é verificada também na entrada de syscall,
para atender processos que passam pouco tempo em modo usuário.

Em QEMU/TCG com quatro vCPUs, BIOS e UEFI recusaram uma tabela alternativa
com `EPERM`. O ELF de teste assinado confirmou UID 1000. Um segundo processo
teve o bind recusado com `EACCES`, consultou o endpoint SEQPACKET e recebeu
o registro POSIX esperado. O console parou o backend enquanto ele repetia
`accept4`. [QEMU BIOS](logs/dircache-supervisionado-bios.log) ·
[QEMU UEFI](logs/dircache-supervisionado-uefi.log)

O host passou 259 testes do kernel e cinco do `pegasusd`, incluindo
o checkpoint de diagnóstico e a troca de registros em SEQPACKET.
[Kernel](logs/dircache-supervisionado-host-kernel.log) ·
[Pegasus DS](logs/dircache-supervisionado-host-pegasus.log)

O binário estático do `pegasusd` compilou. A VM com a reserva de contas
no primeiro disco AHCI não montou o ext4 do segundo disco, e o ELF de
1,2 MiB excedeu o limite de 16 KiB por arquivo do overlay dessa imagem.
A prova no kernel usou um backend sintético. A execução do daemon no perfil
de serviço, o listener LDAP com TLS, o backend transacional e NSS/PAM
continuam pendentes. O acesso ao NAS agora usa `nas-manjaro` ou
`nas-manjaro-cabo`. Ambos passaram a verificação SSH da chave do host,
retornaram o hostname `nas` e deram acesso de leitura a `/dev/kvm`.
A prova do backend sintético passou em BIOS e UEFI também no NAS/KVM.
O recorte da revisão e os resultados por ambiente estão no
[relatório de validação do dircache](validacao-dircache.md).

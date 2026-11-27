# Revisão de engenharia do CarlioOS — 26/11/2026

O CarlioOS tem evidência de boot e execução isolada em VM, além de TCP e supervisão de um processo de teste. O aceite do beta permanece aberto. A revisão cruzou código, critérios e logs. Os achados abaixo distinguem defeitos observados, lacunas de implementação e riscos que ainda exigem teste.

## Achados e ordem de trabalho

| Prioridade | Achado | Ação e critério de verificação |
| --- | --- | --- |
| P0 · corrigido | O perfil `storage-test` reutilizava o nome da ISO padrão. Nessa compilação, o comando serial `diskteste` grava o LBA 1 e os dois últimos setores do dispositivo AHCI escolhido pelo driver. Era possível entregar uma ISO com comandos de escrita de diagnóstico sob o nome usado para boot normal. O perfil de falhas também reutilizava esse nome. [Build](../scripts/build-iso.sh) · [AHCI](../src/arch/x86_64/ahci.rs) | Os nomes agora terminam em `-storage-test.iso` ou `-crash-test.iso`. O build de armazenamento gerou a ISO dedicada e preservou o SHA-256 da ISO padrão. |
| P1 · bloqueio de serviço | OpenSSH abre o listener e inicia `sshd-session`, que para em `setitimer`. A prova local com prazo externo para diagnóstico chega à separação de privilégios e falha em `setrlimit(RLIMIT_FSIZE)`. A limpeza também encontra `kill` indisponível. [Logs e procedimento](../docs/TESTES-NAS.md) | Implementar temporizador, sinais, limites efetivos e isolamento exigidos pelo daemon. Validar que a autenticação expira, que a sessão recebe a identidade correta e que os limites restringem as operações antes de aceitar SSH e SFTP. |
| P1 · descritores, correção parcial | Arquivos abertos guardam o caminho. Operações como leitura, `fstat`, `fchmod` e `fchown` resolvem esse caminho novamente. Antes, `unlink` comparava apenas o texto do caminho. [Chamadas do kernel](../src/main.rs) · [Tabela de arquivos](../src/open_files.rs) | Após verificar as permissões do chamador, `unlink` compara o nó resolvido com o de cada caminho aberto e devolve `EBUSY` quando coincidem. A identidade persistente do objeto na abertura e a tabela comum de descritores seguem abertas. |
| P1 · isolamento de memória | Pilhas de processo são buffers contíguos do kernel com sentinela opcional. O histórico já registra sobreposição de uma fatia IRQ por um prólogo maior que a fatia. A fixture foi movida para uma pilha dedicada. [Pilhas](../src/process.rs) · [Histórico](../docs/ROADMAP.md) | Mapear páginas de guarda para pilhas de kernel e verificar limites de IRQ e syscall com SMP. A sentinela mede consumo observado. |
| P1 · dados e recuperação | O overlay usa dois bancos com CRC. Um erro de leitura no cabeçalho ou conteúdo encerra a abertura imediatamente, mesmo quando o outro banco pode estar legível. O banco persistente também guarda metadados das contas. CRC detecta corrupção acidental e o mecanismo não autentica mídia modificada externamente. [Armazenamento VFS](../src/vfs_store.rs) · [Login](../src/console_session.rs) | Testar erro de leitura separado de corte de energia. Definir recuperação administrativa que preserve revogações e permissões sem reativar automaticamente credenciais antigas. |
| P1 · hardware e rede | A evidência de rede cobre Intel 82574L emulada. O NAS físico usa Realtek RTL8111/8168. A reserva de persistência e os caminhos de timeout, remoção e escrita AHCI ainda precisam de validação física em mídia descartável. [Matriz de hardware](../docs/HARDWARE-BASE.md) · [Teste físico](../docs/TESTE-FISICO.md) | Validar link, DHCP, TCP e recuperação com o driver físico. Completar ensaios AHCI antes de confiar dados do NAS ao kernel. |
| P2 · entropia e disponibilidade | O caminho de entropia depende de RDRAND com tentativas limitadas. Sem essa capacidade, `getrandom` e a abertura de TCP de saída podem devolver `EAGAIN`, e a construção da pilha ELF pode omitir `AT_RANDOM`. [Entropia](../src/arch/x86_64/entropy.rs) · [Integração](../src/main.rs) | Definir suporte por hardware e comportamento de boot quando faltar entropia. Testar com RDRAND desabilitado e manter falha segura quando faltarem bytes aleatórios. |
| P2 · escala e compatibilidade | O executável, a VM e o overlay têm cotas fixas. A capacidade permitiu carregar o OpenSSH na fixture, porém o Samba ainda precisa de um boot com suas dependências no guest. Faltam evidências de SMB por conta, reinício e recuperação. [ABI](../docs/KERNEL_ABI.md) · [Gate de serviços](../docs/TESTES-NAS.md) | Medir tamanho de arquivos, páginas residentes e regiões no boot do Samba. Validar credenciais e limites antes dos testes de leitura, escrita e reinício com `smbclient`. |
| P2 · cadeia de entrega, correção parcial | A build usa nightly flutuante e Limine do host. Agora cada execução prepara a ISO em um diretório temporário próprio, com bloqueio durante a build e publicação somente após a instalação do boot BIOS. Os perfis disk-exec e disk-read geram imagens dedicadas. [Toolchain](../rust-toolchain.toml) · [Build](../scripts/build-iso.sh) · [Instalador](../scripts/install-virtual-image.py) | Fixar versões para o beta, registrar a identidade de cada artefato e validar atualização, rollback e boot verificado. |

## Critérios por bloco

| Bloco | Evidência e trabalho aberto |
| --- | --- |
| 0 e 1 | Contrato e matriz definidos. Boot, diagnóstico e PCI/AHCI de prova validados em VM. Falta completar a matriz física. |
| 2 | CPL3, preempção, TLB remoto e reaper exercitados em VM. Páginas de guarda e revalidação de regressões SMP seguem em aberto. |
| 3 | Ext4/FAT32 nativos montados em leitura. O overlay persistente, corte simulado e AHCI foram exercitados em VM. Swap e provisionamento são experimentais. |
| 4 | Contas locais, execução autenticada, TCP e supervisão de fixture passaram nos respectivos testes. DNS para aplicações, SSH/SFTP, SMB e rede física continuam abertos. |
| 5 | Execução de ELF estático e carregamento de ELF dinâmico já atingiram aplicações específicas. Pacotes e boot de userland Arch/Ubuntu, glibc geral, sinais e threads continuam em aberto. |
| 6 | Diagnóstico headless disponível. Desktop, entrada, DRM/KMS, compositor e integração de agentes continuam abertos. |
| 7 | Preparador de imagem virtual disponível. Instalação física, boot verificado, atualização e rollback aguardam aceite. |
| 8 | Parser PE32+ e materialização de seções disponíveis no host. Execução e APIs Win32/NT permanecem pós-beta. |

## Evidência desta revisão

- A prova CPL3 de VFS confirmou `EBUSY` ao remover um arquivo aberto por caminho equivalente e a remoção após `close`. [BIOS](logs/beta-vfs-alias-qemu-bios.log) · [UEFI](logs/beta-vfs-alias-qemu-uefi.log)
- A compilação de disk-exec e uma falha simulada de `limine bios-install` preservaram o SHA-256 da ISO padrão. O diretório temporário da build com falha foi removido.
- O boot da ISO padrão e o teste de TCP bloqueante passaram no NAS/KVM em BIOS e UEFI com o disco de 20 GiB em snapshot. [Procedimento e logs](../docs/TESTES-NAS.md)
- `cargo test --offline` passou 253 testes no host.
- As ISOs dedicadas de armazenamento e de falhas foram construídas localmente. O SHA-256 da ISO padrão permaneceu idêntico após cada build.
- Os estados de QEMU e NAS do painel citam os logs existentes de cada ensaio. Os marcadores valem para as revisões e configurações registradas.
- A revisão de código foi dirigida aos caminhos críticos de confiança, execução e recuperação. Ela não incluiu auditoria linha a linha de todo o repositório, CVEs das dependências, fuzzing de todos os parsers ou validação de segurança em hardware.

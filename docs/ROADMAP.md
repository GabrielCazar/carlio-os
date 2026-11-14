# Roadmap do CarlioOS

## Princípios

- Código do sistema em Rust `no_std`; Limine, firmware e QEMU são ferramentas
  externas de inicialização/execução.
- Idioma padrão pt_BR em menus, mensagens de inicialização, núcleo, terminal e
  interface. Mensagens serial usam UTF-8; comandos são ASCII sem acentos para
  funcionar em consoles antigos.
- Alvo inicial x86_64 com boot BIOS/UEFI via Limine. QEMU TCG valida fallback e
  KVM valida aceleração. CarlioOS é um sistema convidado, não um hypervisor.
- Perfil básico para servidores legados, com recursos opcionais detectados em
  máquinas atuais. ARM64 é um alvo futuro independente, não compatibilidade já
  entregue.
- SQLite integrado será avaliado depois de existir allocator, filesystem e
  isolamento. Banco não é requisito do boot e não guarda segredos.
- Formatos de pacotes e execução são camadas distintas. Compatibilidade só será
  declarada por formato e versão após suíte de testes.

## Fases e critérios de aceite

## Regra obrigatória de testes

Toda mudança de comportamento executa os testes locais aplicáveis e
`scripts/test-nas.sh`: testes host no NAS e boot da mesma ISO em QEMU/KVM. A
saída e o log serial ficam em `reports/logs/`; resultado ausente ou falho
bloqueia marcar a mudança como validada. O servidor de teste confirmado é o NAS
x86_64, acessado pelo alias SSH `nas-wifi`.

### Fase 1 — Inicialização e núcleo mínimo (concluída no escopo de boot e bloco QEMU)

Boot Limine x86_64, saída serial UTF-8 em pt_BR, terminal com comandos básicos,
modelo documentado de permissões e testes host. A fase foi reaberta para incluir
a base de kernel e o driver de disco necessários à fase 2: mapa de memória
fornecido pelo bootloader, HHDM, descoberta PCI e drivers em etapas para VGA,
SATA/AHCI, USB xHCI/EHCI, Bluetooth USB, Wi-Fi Intel e Ethernet Intel/Realtek.
O AHCI deve ler e escrever setores em disco de teste no QEMU. A prioridade dos
drivers segue o inventário em `docs/HARDWARE-BASE.md`. Aceite: build
reproduzível, boot BIOS e UEFI, saída serial e comandos registrados em log;
detecção positiva e negativa do dispositivo, leitura/escrita conferidas e
falhas de I/O reportadas sem travar o kernel. O teste remoto deve repetir a
mesma ISO no NAS/KVM. A política de permissões atual não é isolamento real; o
kernel roda inteiro no nível de privilégio 0. Os clientes remotos da fase 6
também dependem da futura pilha de rede TCP/IP; detectar NIC não equivale a ter
conectividade de rede.

### Fase 2 — Armazenamento protegido (iniciada em protótipo)

Camada de bloco AHCI QEMU experimental validada em disco descartável; próxima
etapa é estabilizar a interface e os erros. Depois: PCI/VirtIO block, fonte de
entropia, volume versionado e criptografado,
desbloqueio no boot, Hermione em Rust inspirado em ZFS (copy-on-write, checksums,
snapshots, scrub periódico e reparo com redundância) e SQLite integrado. Chaves
não serão persistidas. Aceite: senha válida abre, corrupção é detectada,
redundância recupera blocos testados e ausência de cópia boa é reportada sem
falso reparo; transações/snapshots resistem a reinício. O volume cifra dados
desligados, não protege contra kernel comprometido enquanto aberto. O formato
Hermione não será compatível com pools ZFS existentes sem uma implementação
completa adicional.

### Fase 3 — Segurança e contas

IDT/exceções, paginação e memória por processo, ring 3, scheduler, syscalls
validadas, contas e permissões por capability. Preparar acesso seguro a
credenciais de rede para conexões remotas, usando o armazenamento protegido da
fase 2. O gerenciador de arquivos da fase 6 dependerá dessas permissões e
proteções. Aceite: testes de acesso negado, isolamento entre processos, entrada
inválida e queda segura; auditoria de permissões antes de habilitar dados reais.

### Fase 4 — Aplicações e pacotes Linux

Inspecionar e instalar AppImage, Snap, Flatpak, DEB, RPM e pacotes Arch sem
executar scripts privilegiados. Implementar um subconjunto da ABI Linux em
sandbox apenas quando houver processos e syscalls. O gerenciador de serviços
planejado para a userland compatível com Linux será o **systemd**; sua execução
depende da ABI Linux necessária e dos recursos de kernel que ele exige, e ainda
não está implementada. Aceite: matriz de formatos e versões,
assinaturas/verificação de origem, rollback e suíte de conformidade. Formato
instalado não significa aplicação executável.

### Fase 5 — Windows Mode corporativo

Executar Windows licenciado em VM isolada e integrar domínio AD DS por DNS,
Kerberos e LDAP. Duo Desktop e CrowdStrike Falcon serão instalados e testados
dentro do Windows convidado suportado; agentes dependem do kernel, drivers e
cadeia de confiança do próprio Windows. Não prometer execução desses agentes
via Wine nem reimplementá-los. Aceite: boot da VM, Secure Boot/TPM virtual,
integração com domínio de teste e telemetria validada no tenant autorizado.
Consulte os [requisitos oficiais do Duo](https://duo.com/docs/duo-desktop) e a
[matriz vigente do CrowdStrike](https://www.crowdstrike.com/en-us/products/faq/).

### Fase 6 — Interface e plataformas

Construir um ambiente desktop baseado no **KDE Plasma**, com elementos visuais e
de interação do **Budgie** e barra superior inspirada na do **Omarchy**. Criar
um gerenciador de arquivos nativo com conexões **SSH/SFTP/SCP, SMB, FTP, HTTP e
HTTPS**. SSH/SFTP e SMB devem usar as credenciais protegidas e permissões da
fase 3; todos os protocolos dependem da pilha de rede e drivers NIC previstos
na fase 1. HTTP/HTTPS no gerenciador servem para abrir/baixar recursos, sem
implicar compatibilidade geral com aplicações web. Também implementar
framebuffer, teclado, mouse, imagens, ACPI/APIC e drivers adicionais; perfis de
máquina legada e moderna; depois suporte ARM64. Aceite: matriz QEMU por
firmware/arquitetura/memória/dispositivo, operação headless, fallback sem GPU,
testes positivos/negativos de autenticação e permissões, e logs em pt_BR para
cada perfil.

## Alegações de segurança

Até as fases correspondentes passarem seus critérios e revisão, CarlioOS é
experimental. Uma senha de volume não cifra a imagem do kernel. Boot seguro
exige validar toda a cadeia de confiança, desde firmware/bootloader até kernel
e atualizações assinadas.

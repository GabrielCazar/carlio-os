# Relatório — Fase 6: interface e plataformas

**Estado:** diagnóstico serial disponível. Login gráfico, gerenciador de
arquivos e agentes dependem da ABI Linux, entrada gráfica e rede.

## Escopo

Ambiente desktop baseado no KDE Plasma, com elementos de interface do Budgie e
barra superior inspirada na do Omarchy. Criar um gerenciador de arquivos nativo
com conexões SSH/SFTP/SCP, SMB, FTP, HTTP e HTTPS; conexões autenticadas usam as
credenciais protegidas e permissões definidas na fase 3, e a rede depende dos
drivers e da futura pilha TCP/IP previstos na fase 1. O suporte HTTP/HTTPS no gerenciador é
para abrir e transferir arquivos, não promete executar qualquer aplicação web.

Também implementar framebuffer, teclado, mouse, imagens, ACPI/APIC, drivers
VirtIO adicionais e perfis para hardware legado/moderno. Avaliar suporte ARM64
como implementação de arquitetura separada. Manter mensagens de inicialização
e interface em pt_BR.

## Checklist desta fase

- [ ] Definir identidade visual Plasma com elementos selecionados do Budgie.
- [ ] Implementar barra superior inspirada no Omarchy.
- [ ] Implementar gerenciador de arquivos nativo e operações locais.
- [ ] Integrar conexões SSH e transferência de arquivos SFTP/SCP.
- [ ] Integrar compartilhamentos SMB com autenticação e permissões.
- [ ] Integrar FTP e transferências HTTP/HTTPS, tratando FTP sem criptografia
  como protocolo inseguro e deixando isso visível ao usuário.
- [ ] Validar rede, credenciais, acesso negado e falhas de conexão em ambiente
  de teste; garantir que segredos não apareçam em logs.
- [ ] Implementar framebuffer, teclado e mouse antes de declarar desktop pronto.

## Critérios de aceite

Matriz de boot por firmware, arquitetura, memória e dispositivos; operação
headless; fallback sem GPU; entrada de teclado/mouse; teste visual do desktop e
gerenciador de arquivos; validação de cada protocolo e permissões; logs e
relatórios para cada perfil.

## Estado comprovado

No terminal serial, `diagnostico` mostra o tipo do sistema de arquivos raiz,
o estado do log persistente, os frames livres de 4 KiB, a disponibilidade
do framebuffer de firmware e o tempo desde a inicialização. A consulta usa
apenas o estado do kernel e foi exercitada em NAS/KVM com BIOS e UEFI,
com a saída do QEMU via serial.

Não há interface gráfica, mouse, teclado, imagens, gerenciador de arquivos
remoto ou suporte ARM64. Há framebuffer RGB de firmware com desenho por CPU e
um buffer de texto VGA em memória; ainda não há modesetting Intel nem desktop.
Plasma/Budgie/Omarchy e os protocolos SSH/SFTP, SMB, FTP, HTTP e HTTPS seguem
planejados. Consulte o [relatório de vídeo](fase-video.md).

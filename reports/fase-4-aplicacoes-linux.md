# Relatório — Fase 4: aplicações e pacotes Linux

**Estado:** planejada; depende de processos isolados e armazenamento.

## Escopo

Tratar AppImage, Snap, Flatpak, DEB, RPM e pacotes Arch como formatos distintos.
Separar inspeção/instalação da execução; avaliar ABI e syscalls Linux dentro de
sandbox. Não executar scripts de instalação como administrador por padrão.

## Gerenciador de serviços planejado

O gerenciador de serviços da userland compatível com Linux será o **systemd**.
Essa etapa depende da ABI Linux necessária, incluindo os serviços e interfaces
de kernel exigidos pelo systemd, além do modelo de processos e isolamento da
fase 3. A escolha é arquitetural; systemd ainda não está portado nem executável
no CarlioOS.

## Critérios de aceite

Matriz pública de suporte por formato/versão; integridade e origem verificadas;
rollback testado; suíte de aplicações representativa executa isolada e testes
negativos provam que não acessa recursos fora das permissões.

## Estado comprovado

Não há parser de pacotes, runtime Linux, ABI, syscalls ou sandbox.

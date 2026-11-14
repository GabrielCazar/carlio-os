# Relatório — Fase 3: segurança, contas e isolamento

**Estado:** planejada; depende do núcleo e da memória virtual.

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

O protótipo atual executa inteiramente em ring 0; não há processos, contas ou
controle de acesso efetivo.

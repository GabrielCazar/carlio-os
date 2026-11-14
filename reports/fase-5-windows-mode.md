# Relatório — Fase 5: Windows Mode corporativo

**Estado:** planejada; depende de armazenamento protegido e isolamento.

## Abordagem

Executar Windows licenciado em VM isolada. Integrar domínio AD DS por DNS,
Kerberos e LDAP. Instalar Duo Desktop e CrowdStrike Falcon no Windows convidado
compatível com as matrizes atuais dos fornecedores. Não prometer execução via
Wine nem emular agentes que dependem do kernel e de drivers Windows.

## Critérios de aceite

VM inicia com Secure Boot e TPM virtual; integração a domínio AD DS de teste;
Duo/Falcon instalados e reportando ao tenant autorizado; validar atualizações,
rede, snapshots, recuperação e isolamento da VM.

## Referências oficiais

- [Duo Desktop: sistemas e máquinas virtuais suportados](https://duo.com/docs/duo-desktop)
- [CrowdStrike: matriz de sistemas suportados](https://www.crowdstrike.com/en-us/products/faq/)

## Estado comprovado

Não há hypervisor/gerenciador de VMs, Windows convidado, AD DS, Duo ou Falcon.

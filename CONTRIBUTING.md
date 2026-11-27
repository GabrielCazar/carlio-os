# Contribuindo com CarlIO OS

Siga o [Código de Conduta](CODE_OF_CONDUCT.md). Relate problemas com
passos reproduzíveis e envie informações sensíveis aos mantenedores em privado.

## Preparação

- Leia [AGENTS.md](AGENTS.md), [CORRECT.md](CORRECT.md) e o [roadmap](ROADMAP.md).
- Use os comandos de construção e validação do [README](README.md).
- Vincule a alteração à fase e à interface afetada. Para integrações com Pegasus
  DS ou Pegasus FS, registre os contratos e as revisões testadas.

## Revisão e evidências

- Inclua reprodução, resultado esperado e observado, revisão, ambiente e logs.
- Execute os gates aplicáveis no host, no QEMU local e no NAS. Em mudanças de
  boot, valide BIOS e UEFI. Use imagens descartáveis para testes de escrita.
- Registre cenários positivos, negativos e de falha no relatório da fase, com
  links para `reports/logs/`. Atualize a matriz de pacotes quando necessário.
- Em correções do kernel, use `fix(<área>): <descrição>` e siga o checklist de
  [CORRECT.md](CORRECT.md).

## Licença

GPL-3.0-only. Consulte o [arquivo LICENSE](LICENSE).

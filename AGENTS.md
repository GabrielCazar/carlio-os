# Regras do projeto CarlioOS

- O código do sistema operacional deve ser Rust. Mensagens visíveis no boot,
  kernel e terminal devem estar em português do Brasil (pt_BR).
- O sistema deve ser compativel com aplicações e pacotes linux(deb, rpm, flatpak, snap, pacman, yay, aur e windows).
- Para cada alteração que afete comportamento, segurança, armazenamento,
  inicialização, drivers ou interface, executar testes host locais e
  `scripts/test-nas.sh` antes de declarar sucesso.
- Para toda fase, anexar saída de teste e log serial em `reports/logs/` e
  atualizar o relatório Markdown da fase.
- Não marcar boot, permissões, criptografia, reparo ou compatibilidade como
  implementados antes de testes positivos e negativos no QEMU local e no NAS.
- O NAS deve ser acessado pelo alias `nas-wifi` (ou por
  `CARLIO_TEST_SSH_TARGET`); usar somente arquivos temporários para os testes.
- Nunca salvar senhas/chaves em código, logs, documentação ou argumentos de
  linha de comando.


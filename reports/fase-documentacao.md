# Relatório — documentação operacional

### Correção — consolidação das regras do agente — 16/11/2026

- **Escopo:** atualização de `AGENTS.md` e `CORRECT.md` com roteamento por área,
  invariantes, gates, proibições, referências obrigatórias e formato de resposta.
- **Causa:** instruções acumuladas na sessão precisavam ser consolidadas nos
  arquivos operacionais do repositório.
- **Validação documental:** `git diff --check -- AGENTS.md` passou; verificação de
  newline final e whitespace final passou para `AGENTS.md` e `CORRECT.md`.
- **Testes host, QEMU, NAS, serial e hardware físico:** não aplicáveis; a mudança
  é exclusivamente documental e não altera comportamento do sistema.
- **Logs:** [docs-regras-agents-correct-20261116.txt](logs/docs-regras-agents-correct-20261116.txt).
- **ROADMAP / matriz de hardware / matriz de pacotes:** sem alteração de estado
  de subsistema; não aplicável.
- **Checklist de código:** itens de build, testes e boot não aplicáveis a esta
  alteração documental.
- **Estado:** documentação atualizada; sem alegações de suporte ou mudança de
  estado de fase do produto.

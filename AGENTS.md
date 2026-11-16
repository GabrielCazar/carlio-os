# Contrato de trabalho do CarlioOS

Estas regras são obrigatórias para todo trabalho neste repositório. Antes de
agir, classifique a tarefa pela área (§1 de `CORRECT.md`) e leia as referências
aplicáveis listadas em §11. Em conflito, vale a regra explícita mais restritiva
e recente; nunca enfraqueça um gate para obter aprovação.

## Projeto

- Kernel e componentes próprios em Rust, alvo x86-64 ponta a ponta.
- Mensagens visíveis no boot, kernel e terminal em português do Brasil (`pt_BR`).
- Nunca truncar endereços ou tamanhos: conversões estreitas exigem `try_into()`
  e validação de faixa.
- Segredos não podem aparecer em código, logs, documentação ou argumentos de
  linha de comando.

## Gates e evidências

- Não declarar recurso “funcionando”, “implementado” ou “suportado” sem logs
  positivos, negativos e de falha em `reports/logs/`, referenciados no relatório
  da fase.
- Boot só está funcionando com logs BIOS e UEFI anexados. Panic, `#UD`, watchdog
  e double fault exigem crash-tests intencionais em build de diagnóstico.
- Timeout de serial é finito; saída serial é best-effort e falha de serial nunca
  bloqueia o kernel.
- Mudanças de comportamento, segurança, armazenamento, boot, drivers ou UI
  exigem gates host, QEMU local e NAS aplicáveis. Falha de conectividade NAS:
  registrar “NAS pendente” com log, não repetir em loop e continuar os gates
  locais.
- Memória/SMP, processos/CPL3, storage e ABI/pacotes seguem os gates e invariantes
  específicos de `CORRECT.md` §§2–6.
- Imagens locais descartáveis somente em `/tmp`. I/O de teste no NAS está
  autorizado somente em arquivos temporários sob `/tmp/carlioos-*`, via
  `nas-wifi` ou `CARLIO_TEST_SSH_TARGET`; nunca ler dados do usuário, acessar
  caminhos fora desse diretório ou tocar dispositivos físicos. Não reutilizar
  QCOW2. `storage-test` nunca roda em hardware físico. Se o teste não puder ser
  confinado a `/tmp/carlioos-*`, registrar “NAS pendente”.
- MQ01 é uma exceção de escrita somente após serial exato confirmado, backup
  verificado por hash, rollback testado em QCOW2 descartável e confirmação de
  que não é disco de boot. Até cumprir todos os gates, sem escrita. Os demais
  discos de dados ficam bloqueados em código. Inventário de drivers e
  dispositivos é read-only.

## Segurança e implementação

- Ponteiros de syscall exigem validação de faixa e alinhamento.
- `mmap`/`munmap`/`brk` devem manter VMA, ownership das folhas e page tables
  consistentes. Falha de processo encerra e recolhe recursos sem corromper o
  kernel.
- Todo `unsafe` requer comentário que justifique a invariante protegida.
- Drivers e IRQs não usam `unwrap`, `expect` ou `panic`; usar erros tipados e
  timeouts finitos. Falha de dispositivo não essencial não pode derrubar o
  kernel.
- Usar `IrqSpinLock` nos caminhos de scheduler, pool e heap; nunca segurar lock
  durante I/O.
- Allocator cobre até 32 GiB; memória acima do limite é ignorada e contabilizada.
- `SmpPageTableManager` serializa map/unmap e requer `TlbShootdown` explícito.
  Swap só após VM estável, bloco confiável e testes de falha.

## Pacotes e documentação

- Reconhecimento, instalação, execução, isolamento e atualização de cada formato
  são estados separados em `docs/MATRIZ-PACOTES.md`.
- Scriptlets exigem sandbox e política explícitos; AUR exige sandbox de build;
  Flatpak/Snap exigem isolamento real no kernel.
- Não marcar checklist `[x]` sem link para log; não concluir fase sem todos os
  gates. QEMU sozinho não permite declarar suporte no roadmap.
- Antes de concluir uma alteração, anexar logs, atualizar o relatório da fase e
  revisar `docs/ROADMAP.md`, `docs/HARDWARE-BASE.md` e
  `docs/MATRIZ-PACOTES.md` quando afetados.
- Aplicar o checklist pré-commit de §9 de `CORRECT.md`; commit no formato
  `fix(<área>): <descrição>` com links de evidência no corpo, sem incluir
  alterações alheias.

## Resposta

Usar o formato: **Contexto** (1 linha), **Decisão** (1 linha), **Fluxo**
(Mermaid quando houver decisão/gate/estado), **Artefatos** (arquivos/logs) e
**Próximo gate** (qual e onde o log será anexado). Compactar o resumo em até
cinco tópicos, declarar gates faltantes e recusar ações incompatíveis com as
proibições de `CORRECT.md` §10.

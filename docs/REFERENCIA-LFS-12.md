# Referência de suporte: Linux From Scratch 12.0

Este documento registra como o livro **Linux From Scratch 12.0, edição em
português**, orienta decisões do CarlioOS. O LFS descreve a construção de um
sistema Linux a partir dos fontes; ele não é um manual de implementação de
kernel independente e não especifica o protocolo de troca de contexto x86_64.
Logo, serve como referência para toolchain, organização e compatibilidade de
userland, sem substituir os manuais de arquitetura usados no kernel.

## Aplicação ao CarlioOS

- **Compilação e reprodutibilidade:** os capítulos 5–7 separam toolchain
  cruzada, ferramentas temporárias e ambiente final isolado. Isso orienta a
  futura cadeia de compilação de programas CarlioOS e builds reproduzíveis.
- **Compatibilidade de userland:** a introdução relaciona LFS a POSIX.1-2008,
  FHS 3.0 e LSB 5.0. Esses padrões são referências de metas; não significam que
  o kernel atual implemente POSIX, ABI Linux ou LSB.
- **Estrutura do sistema:** os capítulos 8–9 documentam pacotes básicos,
  hierarquia, inicialização e configuração. Servem para planejar layout de
  arquivos, serviços e utilitários quando houver syscalls, VFS e userland
  suficientes.
- **Pacotes e dependências:** as listas de pacotes e instruções de construção
  ajudam a mapear dependências de uma distribuição de userland. Não tornam
  binários Linux executáveis no CarlioOS por si só.

## Limites para a etapa de execução

O livro não define a ABI interna de interrupções, ownership de pilhas de kernel,
layout de frame `iretq`, paginação x86_64, troca de CR3 nem carregamento seguro de
segmentos ELF. Essas partes continuam sendo implementadas e validadas no código
do kernel e nos testes QEMU. Em particular, suportar o formato ELF não equivale
a suportar executáveis Linux: ainda são necessários ABI de syscalls, linker
dinâmico, bibliotecas e demais contratos de userland.

## Fonte

Gerard Beekmans e Bruce Dubbs (ed.), *Linux From Scratch*, versão 12.0,
publicado em 1º de setembro de 2023. [Livro oficial em português (PDF)](https://www.br.linuxfromscratch.org/lfs/downloads/12.0/LFS-BOOK-12.0-pt_BR.pdf).

# Meta pós-beta — aplicações Windows nativas

## Objetivo

Executar uma matriz selecionada de aplicações Windows x86_64 diretamente no
userland do CarlioOS, sem VM. O kernel permanece CarlioOS; compatibilidade com
PE/COFF e APIs Win32/NT ficará em processos de userland isolados.

## Etapas e dependências

1. Finalizar processos com troca de contexto, syscalls, isolamento, sinais,
   VFS/permissões e alocação confiável.
2. Implementar carregador PE32+: validação de cabeçalhos/seções, relocação,
   imports, TLS e exceções; rejeitar arquivos malformados antes de mapear.
3. Criar camada inicial de compatibilidade para console, arquivos, memória,
   sincronização e processos, com serviços documentados.
4. Expandir rede e gráficos conforme APIs implementadas e dependências do app.
5. Publicar matriz de aplicações e limites por versão; testar sucesso, falha,
   isolamento e consumo de recursos.

## Limites

Não é execução do kernel NT, não carrega drivers `.sys` e não promete
compatibilidade universal. Aplicações que dependem de drivers Windows ou de
internos não implementados continuam fora da matriz nativa.

## Estado

Primeiro incremento implementado em `src/userland/windows.rs`: parser sem
alocação que aceita somente PE32+ AMD64, limita a tabela a 96 seções, valida
assinaturas, cabeçalhos, entry point executável e limites de seções no arquivo
e na imagem. Testes host cobrem imagem válida, máquina/signatura incompatível,
capacidade de seções, entry point não executável e seção truncada.

Esse parser só reconhece metadados; não mapeia a imagem, faz relocação, resolve
imports, inicializa TLS, fornece exceções/Win32/NT ou executa aplicações. A
validação segue host-only. O caminho continua bloqueado pelos pré-requisitos de
processos e ABI de produção e não inclui drivers `.sys`.

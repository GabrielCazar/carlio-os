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

`PeImage::materialize` prepara o layout virtual PE32+ em um buffer fornecido
pelo chamador. Copia cabeçalhos e seções por RVA e preenche lacunas e caudas
com zeros. Os testes host cobrem seções sobrepostas, destino insuficiente e
preservação do destino quando a validação falha.

O mapa permanece no buffer de preparação. Mapeamento em CPL3, relocação,
imports, TLS, exceções, APIs Win32/NT e execução da matriz de aplicativos
seguem pendentes.

# Meta pós-beta — aplicações Windows nativas

## Objetivo

Executar uma matriz selecionada de aplicações Windows x86_64 diretamente no
userland do CarlioOS. A compatibilidade PE/COFF e Win32/NT ficará em
processos isolados. Uma VM dedicada hospedará o serviço de Registro, acessado
por um canal privado mediado pelo kernel do CarlioOS.

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

O [serviço convidado de Registro](../docs/REGISTRO-WINDOWS.md) já oferece
consulta, gravação e remoção de valores em hives privados voláteis. O supervisor
QEMU para Linux permite testar o convidado pela porta serial privada com o
broker e a semântica inicial das operações Win32 de valores. A execução no
CarlioOS depende do backend de virtualização do kernel, da ligação às
credenciais e à memória de CPL3, dos handles e da resolução dos imports PE.

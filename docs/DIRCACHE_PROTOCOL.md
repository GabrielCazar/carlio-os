# Protocolo opt-in de dircache — v1

O kernel oferece um endpoint experimental AF_UNIX `SOCK_SEQPACKET` no
endereço `/run/carlio/dircache.sock`. Para gerar a imagem de desenvolvimento,
execute `CARLIO_DIRCACHE=1 ./scripts/build-iso.sh`. Na imagem padrão o endpoint
permanece desativado.

## Acesso e ciclo de vida

O kernel só permite o bind a um processo de serviço com UID zero que
tenha recebido, durante o `execve`, o domínio `DirectoryBackend` e a
capability `CAP_DAC_OVERRIDE` para o registro do dircache. Essa concessão
é local ao hook do dircache e não libera acesso geral ao VFS. Executar outro
programa revoga o domínio anterior. `mmap` e `mprotect` recusam novas
permissões de execução nesse domínio.

A build aceita um executável estático, sua assinatura destacada e a chave
pública nas variáveis `CARLIO_DIRCACHE_EXECUTABLE`,
`CARLIO_DIRCACHE_SIGNATURE` e `CARLIO_DIRCACHE_PUBLIC_KEY` (use caminhos
absolutos). Use uma chave Ed25519 confiável e uma assinatura gerada com
`openssl pkeyutl -sign -rawin`. A build verifica a assinatura via OpenSSL e
incorpora a imagem aprovada ao kernel. No carregamento a partir de
`/usr/sbin/pegasusd`, o kernel compara os bytes usados pelo carregador ELF
com a imagem aprovada. Sem esses três insumos ou com o ELF divergente, o
processo não recebe a autorização. A confiança depende da integridade da
build, da chave pública escolhida pelo operador e da cadeia de boot.
O carregamento só concede o domínio via `execve`; binários com interpretador
dinâmico são excluídos da concessão. Esta política de domínio é específica
do dircache e não implementa uma interface geral de módulos LSM Linux.

Cada descritor pertence ao processo que o abriu. O backend é único e
encerramento, exec ou fechamento do listener revogam o registro. O
encerramento do listener invalida sua geração e sinaliza erro às conexões.

O servidor faz bind, listen e accept. O cliente conecta e espera `POLLOUT`
antes de enviar. Cada conexão permite uma consulta pendente e cada leitura
consome um registro. Uma leitura curta descarta o restante do registro.
Operações de fila retornam `EAGAIN` quando precisam ser repetidas, incluindo
quando se omite `SOCK_NONBLOCK`. Use `poll` com timeout zero. A consulta
expira após cinco segundos de tempo monotônico, verificados na próxima
operação de leitura, escrita ou poll. O backend recebe um ticket gerado pelo
kernel. O cliente envia valores não nulos no cabeçalho, como 1 e 1, e recebe
o ticket do kernel na resposta.

## Frames v1

Inteiros little-endian e frames de até 64 KiB. O cabeçalho comum contém
`version:u16` (1), `kind:u16` (1 consulta, 2 resposta), `length:u32`
(tamanho total, incluindo cabeçalho), `request_id:u64` e `generation:u64`.
Na consulta seguem `name_len:u32`, `flags:u32` (zero na v1) e nome UTF-8
de até 255 bytes. Na resposta seguem `status:u32` (0 encontrado, 1 ausente,
2 indisponível), `ttl_ms:u32` (até 60.000) e payload UTF-8. O tamanho do
payload é o comprimento total menos os 32 bytes de cabeçalho.

Quando encontrado, o registro usa campos separados por tabulação: nome
igual ao consultado, UID, GID e até 32 GIDs suplementares. IDs são decimais
de 32 bits e o valor reservado `u32::MAX` é rejeitado. Uma resposta de
ausência ou indisponibilidade tem payload vazio. Indisponibilidade também
exige TTL zero. Identidades retornadas são dados de diretório.
Timeout é limitado pela política de instalação; timeout, frame malformado,
versão desconhecida e backend ausente falham fechados, sem fallback para
identidade privilegiada.

O console do kernel expõe em `logs` os últimos 64 eventos do dircache.
O buffer volátil registra PID, operação e errno, incluindo registro, remoção,
consulta, resposta, timeout e falhas do broker. Nomes, conteúdo de registros
e credenciais ficam fora do log. Com uma reserva AHCI de log provisionada,
o kernel grava os eventos disponíveis no log circular existente, no formato
`DIRAUD01`. O comando `logs` também exibe os eventos do dircache nos últimos
64 registros do disco. `/dev/carlio/audit` é um dispositivo virtual de
leitura exclusiva para UID zero, com fechamento automático em exec.
Ele entrega o buffer RAM da inicialização atual como registros binários
de 36 bytes. Cada registro contém a marca `DIRAUD01`, sequência e PID
como u64 little-endian, errno como u16 little-endian e operação ASCII
em dez bytes preenchidos com zeros. O descritor começa no primeiro registro
retido e retorna `EOVERFLOW` se ficar para trás. Reabra o dispositivo para
retomar do primeiro registro disponível.
Essa trilha é limitada pela capacidade do disco e do buffer RAM. Em caso de
falha de gravação, a coleta persistente é suspensa e o console informa o
erro. A trilha ainda não é uma condição para a autorização do endpoint.

O transporte experimental limita o registro a 1.024 bytes por pacote, o que
comporta todos os registros v1 com IDs decimais canônicos. O limite de 64 KiB
continua sendo o limite do codec. O adaptador no `pegasusd`, a ponte NSS e
a homologação no NAS/KVM seguem pendentes.

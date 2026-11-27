# Pegasus DS no perfil de serviço do CarlIO OS

O perfil executa o modo `--dircache` de um `pegasusd` estático. O build verifica
a assinatura Ed25519 antes de incorporar o ELF na imagem do kernel. A conta
configurada por `CARLIO_DIRCACHE_SERVICE_UID` precisa ter UID diferente de zero.
Ela inicia e encerra o serviço pela sessão autenticada do console.

## Lançamento em CPL3

```mermaid
flowchart TD
    SIG["Build verifica assinatura do ELF"] --> IMG["Kernel incorpora o ELF aprovado"]
    CLI["Sessão autenticada pede serviço em CPU secundária"] --> POLICY{"UID configurado e argumentos fixos?"}
    POLICY -->|Não| DENY["EPERM"]
    POLICY -->|Sim| LOAD["Loader mapeia o ELF num espaço de endereços próprio"]
    IMG --> LOAD
    LOAD --> CPL["pegasusd em CPL3, sem acesso ao console"]
    CPL --> MAP["Abre /etc/pegasusds/identities.tsv para leitura"]
    CPL --> UNIX["Registra endpoint AF_UNIX SOCK_SEQPACKET"]
```

## Limites do processo

```mermaid
flowchart LR
    DAEMON["pegasusd em CPL3"] --> ENTRY["Entrada comum de SYSCALL e int 0x80"]
    ENTRY --> FILTER{"Operação permitida pelo perfil?"}
    FILTER -->|Não| ERR["EPERM"]
    FILTER -->|Sim| KERNEL["Kernel verifica posse do descritor e argumentos"]
    KERNEL --> VFS["VFS aplica UID, GID e grupos à tabela de identidades"]
    KERNEL --> EP["Broker aplica domínio e capacidade no bind"]
    KERNEL --> VM["Memória privada e permissões de página"]
```

O perfil veda criação de processos, troca de executável, alteração de
credenciais, sockets de rede, conexões iniciadas pelo daemon e mapeamentos
executáveis criados em tempo de execução. O único arquivo que o daemon
pode abrir é a tabela configurada. A escrita exige um descritor do broker ou uma das saídas nulas privadas.
Os três descritores padrão são nulos para satisfazer a inicialização do Rust
sem conectar o daemon à entrada ou à saída do console.
O escalonador e as cotas comuns de memória são compartilhados com o restante
do kernel.

## Consulta local de identidade

```mermaid
sequenceDiagram
    participant C as Cliente CPL3
    participant K as Broker de dircache
    participant D as pegasusd
    participant V as VFS
    D->>V: Lê a tabela de identidades
    D->>K: bind e listen no endpoint fixo
    C->>K: Conecta e envia uma consulta
    K->>D: Entrega um registro SOCK_SEQPACKET
    D->>K: Envia um registro com a resposta
    K->>C: Entrega a resposta ao cliente
    Note over K,D: Encerrar o serviço libera o endpoint
```


## Build e operação

No repositório do Pegasus DS, compile a variante estática GNU para o
modo dircache. Use o ELF `ET_EXEC` produzido por esta configuração.

```sh
export RUSTFLAGS='-C target-feature=+crt-static -C relocation-model=static -C link-arg=-no-pie -C strip=symbols'
cargo +stable build --locked --release -p pegasusd --features dircache
```

Gere a assinatura Ed25519 destacada com a chave da implantação e passe
os caminhos absolutos do ELF, da assinatura e da chave pública na build
do CarlIO OS. Configure o UID da conta que lerá a tabela.

```sh
export CARLIO_DIRCACHE=1 CARLIO_DIRCACHE_SERVICE_UID=1000
export CARLIO_DIRCACHE_EXECUTABLE="$daemon"
export CARLIO_DIRCACHE_SIGNATURE="$assinatura"
export CARLIO_DIRCACHE_PUBLIC_KEY="$chave_publica"
./scripts/build-iso.sh
```

Entre no console com a conta provisionada e inicie o serviço. Use o
PID informado pelo supervisor para a parada.

```text
servico iniciar /usr/sbin/pegasusd --dircache /etc/pegasusds/identities.tsv
servico parar PID
```

[Resultados da validação em VM](../reports/validacao-dircache.md)

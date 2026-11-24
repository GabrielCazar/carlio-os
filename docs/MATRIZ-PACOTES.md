# Matrizes de compatibilidade de pacotes

Este documento separa **reconhecer um formato**, **instalar seus arquivos** e
**executar os programas resultantes**. Um arquivo que pode ser inspecionado não
é, por isso, instalável ou compatível com a ABI do CarlioOS. Os estados abaixo
descrevem o repositório atual; “planejado” não é suporte implementado.

## Estado por formato e operação

| Formato | Metadados | Integridade e origem confiável | Dependências | Instalar/remover | Executar payload | Isolamento | Atualizar/rollback |
|---|---|---|---|---|---|---|---|
| ALPM / pacote Arch | **Parcial** — leitor host de `.PKGINFO` | Pendente — chaveiro, assinatura e política de repositório | Pendente — libalpm/pacman e dependências Arch | Pendente — extração segura e transação reversível | Pendente — ABI Linux suficiente para os binários do pacote | Pendente — permissões e sandbox por processo | Pendente — banco ALPM, transação e rollback |
| DEB | **Parcial** — leitor host de `control` | Pendente — autenticação do índice APT e política de origem | Pendente — dpkg/APT e relações de dependência | Pendente — extração e manutenção do estado dpkg | Pendente — ABI Linux suficiente para os binários do pacote | Pendente — scripts privilegiados isolados e política de processo | Pendente — estado dpkg/APT e rollback |
| RPM | Pendente | Pendente — assinatura e confiança da origem RPM | Pendente — semântica de capacidades e versões RPM | Pendente — banco RPM, scriptlets e transação | Pendente — ABI Linux suficiente para os binários do pacote | Pendente — scriptlets e payload sob política explícita | Pendente — banco RPM e recuperação transacional |
| Flatpak | Pendente | Pendente — assinatura/chaves do repositório e identidade do remoto | Pendente — runtimes, extensões e refs | Pendente — OSTree/OCI, exports e instalação por usuário/sistema | Pendente — ABI, runtime e integração gráfica necessários | Pendente — sandbox, permissões e portais não existem no kernel atual | Pendente — refs versionadas; downgrade/rollback ainda sem implementação |
| Snap | Pendente | Pendente — assertions e confiança do serviço/origem | Pendente — bases, content snaps e interfaces | Pendente — snapd, montagem, estado e serviços | Pendente — ABI e runtime snapd necessários | Pendente — confinement, interfaces, namespaces e filtros ainda ausentes | Pendente — refresh, retenção de revisão e revert |
| AppImage | Pendente | Pendente — validar origem, hash/assinatura quando fornecida | Pendente — dependências embutidas ainda podem exigir ABI/bibliotecas do host | Pendente — catálogo e armazenamento opcional; não há banco nativo de pacotes | Pendente — runtime AppImage, SquashFS/FUSE ou extração e ABI compatível | Pendente — não presumir sandbox; executar com política restrita do CarlioOS | Pendente — atualização é externa/opcional; troca atômica e rollback do catálogo |

**Estado global:** nenhum dos seis formatos está instalado de ponta a ponta e
nenhum executável vindo de pacote foi validado no CarlioOS. Os leitores de
`.PKGINFO` e `control` são bibliotecas host de inspeção, não são `pacman`,
`dpkg` ou instaladores. A entrada Linux `SYSCALL` já passa `getpid` e `exit` na
prova CPL3; isso não satisfaz a ABI de uma aplicação real.

Para Arch, há um perfil de pacotes e um construtor host de rootfs em
[`userland/arch/`](../userland/arch/). Seu resultado não é contabilizado como
instalação no kernel, execução de payload, isolamento ou rollback; essas
operações permanecem pendentes nesta matriz. No host, o snapshot de
2026/11/23 instalou 110 pacotes Arch com assinaturas obrigatórias e zsh
5.9.2 passou em chroot. A preparação Ubuntu Base validou a assinatura da
imagem e recusou um manifesto alterado. A instalação por APT no CarlioOS
permanece pendente. [Evidências](../reports/fase-4-aplicacoes-linux.md#correção--abi-vfs-pt_interp-e-bootstrap-de-userlands--2026-11-24).

## Matriz de arquitetura e pré-requisitos

| Formato | Gerenciador/origem | Conteúdo e estado local | Pré-requisitos CarlioOS antes do suporte útil |
|---|---|---|---|
| ALPM | `pacman`/libalpm; repositórios Arch e, em fase posterior, AUR via ferramenta separada | Tarball comprimido com arquivos e metadados como `.PKGINFO`; banco em `/var/lib/pacman` | ABI Linux, ELF dinâmico, filesystem FHS persistente, parser/extração segura, verificação de repositório, transações e execução controlada de hooks. AUR executa receitas de build e exige sandbox de build separado |
| DEB | `dpkg` mais APT; repositórios Debian/Ubuntu | `.deb` contém dados de instalação e metadados/scripts; banco em `/var/lib/dpkg` | ABI Linux, ELF dinâmico, FHS persistente, verificação autenticada de índices, resolução APT/dpkg, tratamento de conffiles e política/sandbox de maintainer scripts |
| RPM | `rpm` mais um frontend/repositório (por exemplo, DNF) | Lead, assinatura, cabeçalho de metadados e payload; banco RPM próprio | Parser compatível com versões/codificações, verificação criptográfica, semântica de capabilities, transação/banco RPM e política isolada para scriptlets |
| Flatpak | `flatpak`; remotos como Flathub | Aplicativos e runtimes versionados em OSTree ou OCI; estado em `/var/lib/flatpak` e/ou instalação do usuário | ABI Linux, namespaces, mounts, seccomp, cgroups equivalentes, portal de arquivos/desktop, D-Bus controlado, runtimes e exports. Permissões declaradas devem ser aplicadas pelo kernel |
| Snap | `snap`/`snapd`; Snap Store ou origem configurada | Imagem SquashFS e metadados de aplicação/serviço; revisões e dados separados | ABI Linux, mount namespaces, SquashFS, serviço snapd, controle de interfaces, confinamento equivalente a AppArmor/seccomp/cgroups e ciclo seguro de refresh/revert |
| AppImage | Sem gerenciador obrigatório; distribuição direta pelo fornecedor | Executável autocontido; Type 2 usa runtime e filesystem SquashFS montado via FUSE | ABI Linux compatível, SquashFS e FUSE ou modo de extração seguro, catálogo opcional, verificação de origem e sandbox do CarlioOS. O formato sozinho não fornece isolamento nem atualizações gerenciadas |

Os `.deb` binários têm membros de dados e metadados segundo a Debian Policy;
RPM descreve cabeçalho/assinatura/payload; pacotes Arch são arquivos tar com
metadados no arquivo. Flatpak fornece runtimes e permissões de sandbox; Snap
depende de `snapd` e do modelo de confinement; AppImage Type 2 combina runtime
com SquashFS e normalmente monta via FUSE. Essas propriedades vêm dos formatos
e ferramentas upstream, não de funcionalidades já presentes no CarlioOS.

## Ordem de implementação e gates

1. **ALPM/Arch:** prioridade da userland base. Completar ABI dinâmica, VFS
   persistente e banco; validar assinatura de repositório e pacote; implementar
   extração com rejeição de caminhos absolutos, `..`, links perigosos e colisões;
   aplicar transação com recuperação; só então executar um pacote pequeno.
2. **DEB:** reutilizar camada segura de arquivos/transação sem compartilhar o
   banco ALPM. Implementar índices APT autenticados, semântica de versões,
   dependências, conffiles e scripts sob política explícita.
3. **RPM:** adicionar parser/banco e semântica RPM separadamente; testar
   capacidades, dependências versionadas, assinaturas e scriptlets.
4. **Flatpak e Snap:** tratar como runtimes de aplicações, não como extração de
   arquivos no FHS. O gate é o isolamento real no kernel, mounts, IPC e
   permissões; se o kernel não impuser a política, o pacote não pode ser
   anunciado como sandboxed.
5. **AppImage:** validar o formato e sua origem, disponibilizar SquashFS/FUSE ou
   extração isolada, e executar com ABI compatível. Não atribuir sandbox ou
   atualização automática ao arquivo por padrão.

Cada gate exige pelo menos um pacote representativo positivo e casos negativos
para arquivo malformado, caminho de extração hostil, arquitetura incompatível,
dependência ausente e origem/assinatura inválida. Instalação, execução,
isolamento e rollback são resultados separados. O painel só deve marcar uma
operação como concluída após evidência QEMU e, quando afetar o NAS, validação
remota pelo procedimento do projeto.

## Referências dos formatos

- Arch: [criação de pacotes](https://wiki.archlinux.org/title/Creating_packages) e [especificação do pacote](https://wiki.archlinux.org/title/User:Apg#Format_Specifications).
- Debian: [Debian Policy — pacotes binários](https://www.debian.org/doc/debian-policy/ch-binary) e [campos de controle](https://www.debian.org/doc/debian-policy/ch-controlfields).
- RPM: [formato RPM v4](https://rpm.org/docs/latest/manual/format_v4.html) e [dependências em spec](https://rpm.org/docs/latest/manual/spec.html#dependencies).
- Flatpak: [conceitos básicos](https://docs.flatpak.org/en/latest/basic-concepts.html), [permissões de sandbox](https://docs.flatpak.org/en/latest/sandbox-permissions.html) e [implementação interna](https://docs.flatpak.org/en/latest/under-the-hood.html).
- Snap: [confinamento](https://documentation.ubuntu.com/security/security-features/privilege-restriction/snap-confinement/) e [segurança e atualizações](https://documentation.ubuntu.com/core/explanation/security-and-sandboxing/).
- AppImage: [arquitetura](https://docs.appimage.org/reference/architecture.html) e [FUSE/extração](https://docs.appimage.org/user-guide/troubleshooting/fuse.html).

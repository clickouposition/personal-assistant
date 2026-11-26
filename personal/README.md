# Personal Assistant

Versão personalizada do [Handy](https://github.com/cjpais/Handy) (ditado por voz open source, licença MIT) para quem fala **português e inglês** no trabalho.

Um atalho só, **Option+Espaço**, em qualquer app (Slack, WhatsApp, e-mail, Claude...):

| Você fala | O que é colado |
|---|---|
| Em português | Português corrigido, natural, sem ponto final |
| Em inglês | Inglês americano corrigido e casual |
| "**Traduza para o inglês** ..." | Tradução contextual para o inglês |
| "**Traduza para o português** ..." | Tradução contextual para o português |
| "**Traduz** ..." | Inverte o idioma |
| ... e diz "formal" | Tom formal |

O pedido de tradução pode vir **no começo ou no fim** da fala e aceita variações: "traduza do inglês para o português", "translate to Portuguese", "pro inglês", "..., traduz pro inglês".

### Comandos de voz: "Monatela, ..."

Comece a fala com **"Monatela"** e, em vez de colar texto, o Mac executa a ação:

| Você fala | O que acontece |
|---|---|
| "Monatela, abre o Slack no grupo marketing" | Abre o Slack direto na conversa |
| "Monatela, abre o projeto Cliente X no Reaper" | Acha o `.rpp` mais recente com esse nome e abre no REAPER |
| "Monatela, abre o projeto personal assistant no VS Code" | Acha a pasta do projeto e abre no VS Code |
| "Monatela, fecha as abas do Chrome" / "fecha essa aba" / "fecha as outras abas" | Fecha abas (Cmd+Shift+T reabre) |
| "Monatela, abre o Spotify" / "fecha o Spotify" | Abre ou fecha qualquer app instalado |
| "Monatela, abre o WhatsApp na conversa com a Maria" | Abre o WhatsApp direto na conversa |
| "Monatela, manda no WhatsApp pra Maria: chego em 10 minutos" | Abre a conversa e **escreve** a mensagem (não envia) |
| "... chego em 10 minutos **e envia**" | Escreve **e envia**. Só envia se você disser "e envia", "envia agora", "and send" |
| "Monatela, escreve no Slack no grupo marketing que o post está pronto" | Mesmo esquema, no Slack |
| "Monatela, toca Coldplay no Spotify" | Abre a busca no Spotify (clique para tocar) |
| "Monatela, pausa a música" / "próxima música" / "música anterior" | Controla o Spotify |
| "Monatela, volume 30" / "aumenta o volume" / "tira o som" | Volume do Mac |
| "Monatela, coloca o Chrome na esquerda" / "maximiza o VS Code" / "essa janela na direita" | Organiza janelas (esquerda, direita, tela cheia, centro, em cima, embaixo) |
| "Monatela, abre o YouTube" | Abre o site |
| "Monatela, pesquisa previsão do tempo" | Pesquisa no Google |

O resultado aparece numa notificação do macOS. Para escrever mensagens, o texto passa pela área de transferência, que é restaurada logo depois. A IA local **só escolhe** uma ação dessa lista; o código é que executa. Nenhum comando livre é rodado, então uma transcrição errada não apaga nada. Na primeira vez que controlar o Chrome ou o Slack, o macOS pede permissão de **Automação** para o Handy: clique em **OK**.

Projetos são procurados em Mesa, Documentos, Developer, Projects, Música e Downloads (até 3 níveis). Registro de tudo em `~/Library/Logs/com.pais.handy/personal-assistant.log`.

**Slack pelo contexto (opcional, recomendado):** com uma chave só de leitura do Slack, o comando conhece **todos** os seus canais, grupos e DMs, entende de qual você está falando ("Wesley Chapel suporte" → `#lc-wesley-chapel-support`) e abre direto, sem digitar na busca. Sem a chave, ele digita o nome na busca do Slack.

1. Abra https://api.slack.com/apps → **Create New App** → **From a manifest** → escolha o workspace.
2. Cole o conteúdo de [`commands/slack-app-manifest.json`](commands/slack-app-manifest.json) → **Create**.
3. **Install to Workspace** → **Allow** (em alguns workspaces o admin precisa aprovar).
4. Em **OAuth & Permissions**, copie o **User OAuth Token** (começa com `xoxp-`).
5. No Terminal, rode o comando abaixo e cole o token quando ele pedir (ele fica no **Keychain** do Mac, não em arquivo):
   ```bash
   security add-generic-password -a "$USER" -s personal-assistant-slack -U -w
   ```

A lista fica em cache por 6 horas (e é atualizada sozinha se você falar de um canal novo). Para remover: `security delete-generic-password -s personal-assistant-slack`.

**Criar uma ação nova:** em `commands/commands.py`, escreva uma função `act_<nome>(params)`, descreva em `ACTION_HELP` e acrescente o nome no `enum` do `SCHEMA`. Vale na hora, sem recompilar.

### Segurar ou mãos livres

- **Segurar:** mantenha **Option+Espaço** apertado enquanto fala; soltou, finalizou.
- **Mãos livres:** dê um **toque rápido** em Option+Espaço e fale. Quando você **parar de falar por ~1,5 s**, ele finaliza sozinho (ou toque de novo para finalizar antes). Pausas curtas no meio da frase não cortam a gravação.

Tudo roda **no próprio Mac, grátis e offline**: Whisper Large v3 Turbo transcreve, e o Gemma 4 (via Ollama) corrige e traduz.

## Requisitos

- Mac com **Apple Silicon** (M1 ou mais novo), macOS recente
- **16 GB de RAM** recomendado (os dois modelos ficam carregados, uns 11 GB)
- **~20 GB livres** (ferramentas, modelos e compilação)
- Internet durante a instalação

## Instalação (um comando)

```bash
git clone https://github.com/clickouposition/personal-assistant.git
cd personal-assistant
bash personal/install-macos.sh
```

Este repositório é o Handy **v0.10.2** já com as mudanças. O instalador compila e instala a partir dele.

Leva de 15 a 30 minutos, quase tudo é download e compilação. Pode rodar de novo sem problema: ele pula o que já está instalado.

Com uma IA (Claude Code etc.), basta pedir: *"clone github.com/clickouposition/personal-assistant e instale seguindo personal/SKILL.md"*.

Depois, só você pode fazer estas duas coisas:
1. **Ajustes do Sistema → Privacidade e Segurança → Acessibilidade** → ative o **Handy**
2. No primeiro ditado, clique em **Permitir** no pedido de **Microfone**

## Para assistentes de IA (Claude Code, Codex, Cursor...)

Se você é uma IA instalando isto para alguém:

1. Siga o [`SKILL.md`](SKILL.md). Em resumo: na raiz do repositório, rode `bash personal/install-macos.sh`. Ele não pede senha e para com uma mensagem clara se algo faltar.
2. Se ele parar pedindo as **Command Line Tools**, a pessoa precisa clicar em *Instalar* na janela do macOS. Depois rode o script de novo.
3. Você **não consegue** liberar Acessibilidade nem Microfone. Peça para a pessoa fazer (instruções no fim do script). Para confirmar que deu certo, procure `Shortcuts initialized successfully` em `~/Library/Logs/com.pais.handy/handy.log`.
4. Se o log mostrar `SecureInput`, um campo de senha ou o "Entrada de Teclado Segura" do Terminal está bloqueando atalhos. Peça para fechar.
5. Não troque o provedor de IA nem coloque chaves de API sem a pessoa pedir.

## O que foi mudado no Handy

Tudo marcado com `// [custom]` no código:

| Arquivo | Mudança |
|---|---|
| `src-tauri/src/custom_lang.rs` | **Novo.** Detecta pedidos de tradução e o idioma falado, e manda uma ordem clara para a IA (modelos pequenos erram o idioma sozinhos). Tem testes. |
| `src-tauri/src/custom_autostop.rs` | **Novo.** No modo mãos livres, finaliza a gravação sozinho depois de ~1,5 s de silêncio após a fala (com testes). Ligado no coordenador e no detector de voz (`transcription_coordinator.rs`, `audio_toolkit/audio/sink.rs`) |
| `src-tauri/src/custom_commands.rs` | **Novo.** Se a fala começa com "Monatela", não cola: chama `commands/commands.py` (com testes) |
| `src-tauri/src/actions.rs` | Liga a detecção de idioma ao pós-processamento (placeholder `${language_directive}` no prompt) |
| `src-tauri/src/llm_client.rs` | Pede ao Ollama para manter o modelo sempre carregado (`keep_alive`), sem atraso de "modelo frio" |
| `src-tauri/src/settings.rs` | Padrões: atalhos, Ollama + `gemma4:e4b`, prompt "Auto PT/EN", colagem confiável, Whisper sempre carregado, sem atualização automática |

Arquivos desta pasta:
- `SKILL.md`: passo a passo para assistentes de IA
- `install-macos.sh`: o instalador
- `profile.json`: configurações aplicadas pelo instalador (também valem para quem já tinha o Handy)
- `prompts/auto-pt-en.txt`: o prompt de correção/tradução
- `commands/commands.py`: os comandos de voz (lista de ações permitidas)

## Personalizar

- **Prompt:** edite `prompts/auto-pt-en.txt` e rode o instalador de novo (ou cole o texto em Handy → Pós-processamento).
- **Atalhos e opções:** edite `profile.json`.
- **Usar o Claude em vez do Gemma** (melhor qualidade, pago por uso): em Handy → Pós-processamento, escolha *Anthropic* e cole sua chave de API.

## Atualizações

Esta versão **não se atualiza sozinha** (o atualizador trocaria pela versão oficial e apagaria as mudanças). Para pegar novidades do Handy oficial:

```bash
git remote add upstream https://github.com/cjpais/Handy.git   # só na primeira vez
git fetch upstream --tags
git merge v0.11.0              # a versão nova; resolva conflitos nos arquivos marcados com [custom]
bash personal/install-macos.sh
```

Cada nova compilação pede para liberar a **Acessibilidade** de novo. É normal para apps compilados em casa.

## Problemas comuns

| Sintoma | Solução |
|---|---|
| Colou um texto antigo da área de transferência | Já mitigado com "Colagem confiável". Se repetir, aumente `paste_delay_after_ms` em `profile.json` |
| Texto colado sem correção/tradução | O Ollama não está rodando. Abra o Ollama.app |
| Atalho não faz nada | Acessibilidade desligada, ou um campo de senha aberto |
| Muito lento | Feche apps pesados; com menos de 16 GB de RAM, os modelos competem por memória |

## Licença

O Handy é MIT ([licença original](https://github.com/cjpais/Handy/blob/main/LICENSE)), © CJ Pais. As mudanças e os scripts seguem a mesma licença.

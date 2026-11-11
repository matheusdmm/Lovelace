# Lovelace

Gravador de ideias em Rust para Windows. Ao encerrar a gravação, o Lovelace salva o áudio, transcreve a fala e gera uma síntese em Markdown com links para os trechos correspondentes. A inferência roda na máquina.

## Interface desktop

Com o MinGW x64 no PATH e o runtime Microsoft Edge WebView2 instalado, execute na raiz do projeto:

```powershell
cargo run --release --features desktop --bin lovelace-desktop
```

Selecione o idioma e clique em **Gravar**. A interface usa a entrada padrão do Windows. Ao clicar em **Encerrar**, a gravação é salva em `sessoes/` e o processamento continua em segundo plano. Configure os modelos e executáveis descritos abaixo para gerar a transcrição e a síntese. Se o processamento falhar, selecione a gravação no histórico e use **Processar sessão** para tentar novamente.

O histórico também abre sessões criadas pela CLI. Na síntese, clique num tempo para ir ao trecho da transcrição e posicionar o player. **Copiar Markdown** copia o texto original do documento aberto.

## Começar pelo microfone

Na raiz do projeto, com o MinGW x64 (`gcc.exe`) no PATH e a toolchain Rust GNU disponível:

```powershell
cargo run --release -- gravar --so-audio
```

Fale depois da mensagem **Gravando** e pressione Enter para salvar. O terminal mostra o caminho de `audio.wav`. Use `microfones` para listar as entradas e `gravar --microfone 0` para selecionar a entrada de índice 0. Por padrão, o app usa a entrada selecionada no Windows. Se a captura falhar, confira a entrada padrão e a permissão de microfone para aplicativos da área de trabalho nas Configurações do Windows.

Para adicionar a toolchain GNU à instalação existente, use `rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal`.

## Transcrição e síntese

Coloque os executáveis e suas DLLs nas pastas abaixo. Os links apontam para os arquivos dos projetos.

| Caminho esperado | Download |
| --- | --- |
| `engines/whisper/Release/whisper-cli.exe` | [whisper.cpp para Windows x64](https://github.com/ggml-org/whisper.cpp/releases/tag/b5127) |
| `engines/llama/llama-server.exe` | [llama.cpp para Windows x64 com Vulkan](https://github.com/ggml-org/llama.cpp/releases/tag/b12289) |
| `models/ggml-small-q5_1.bin` | [Whisper Small multilíngue quantizado](https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin) |
| `models/qwen2.5-3b-instruct-q4_k_m.gguf` | [Qwen2.5 3B Instruct Q4_K_M](https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/blob/main/qwen2.5-3b-instruct-q4_k_m.gguf) |

O binário oficial de Whisper indicado acima roda em CPU. O de llama.cpp usa Vulkan para a síntese. Se precisar usar CPU nas duas etapas, adicione `--cpu`. Use `--whisper`, `--llama`, `--modelo-whisper` e `--modelo-llama` para apontar para outros arquivos.

```powershell
cargo run --release -- gravar
```

O padrão é PT-BR. Para uma sessão em inglês, use `gravar --idioma en`. Use `--idioma auto` para detectar o idioma predominante. O Whisper transcreve no idioma da fala.

Cada pasta em `sessoes/` contém `audio.wav`, `transcricao.md` e `sintese.md`. Abra a síntese na visualização de Markdown e clique nos tempos ao lado de cada apontamento para ir à seção correspondente da transcrição. O JSON do Whisper fica na sessão para que o app possa repetir a síntese usando os mesmos trechos. Gravações longas são sintetizadas em blocos, então uma ideia retomada em outro bloco pode aparecer em mais de um apontamento.

Para processar uma gravação anterior ou retomar após uma falha, substitua `NOME-DA-SESSAO` pelo nome que o terminal mostrou:

```powershell
cargo run --release -- processar .\sessoes\NOME-DA-SESSAO
```

O comando preserva o WAV e reaproveita o JSON de transcrição existente, incluindo o idioma. Ao processar pela primeira vez uma sessão gravada com `--so-audio`, passe `--idioma en` se a fala estiver em inglês.

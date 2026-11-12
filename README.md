# Lovelace

Lovelace is an idea recorder for Windows and Linux that produces transcripts and summaries in Markdown. Audio and inference stay on your machine. Each summary note links to the transcript segments that support it.

The [Lovelace website](website/) includes downloads, an audit guide, and instructions for building and running the app. See the [website README](website/README.md) for the Coolify configuration, Docker commands, and release link settings.

## Binaries

The [Build binaries workflow](.github/workflows/build.yml) runs on pushes and pull requests to `main`. You can also run it manually from **Actions**. Downloads are available under **Artifacts** for each run.

| x64 artifact | Contents |
| --- | --- |
| `lovelace-windows-x64.zip` | `lovelace.exe`, `lovelace-desktop.exe` and `WebView2Loader.dll` |
| `lovelace-linux-x64.tar.gz` | `lovelace` and `lovelace-desktop`, built on Ubuntu 24.04 |

The Windows desktop app requires Microsoft Edge WebView2 Runtime. On Linux, the binaries use ALSA and the desktop app also requires WebKitGTK 4.1. Extract the archive and run the CLI from the extracted directory. Install the engines and models separately in that directory.

## Requirements

- On Windows, build with Rust 1.89 or later, the `stable-x86_64-pc-windows-gnu` toolchain and x64 MinGW-w64 (`gcc.exe` on `PATH`).
- On Linux, use the native `stable` toolchain and install the build dependencies listed in the workflow.
- For transcription and summarization, install the engines for your platform, their libraries and the models at the paths below.

| Path | Download |
| --- | --- |
| `engines/whisper/Release/whisper-cli.exe` | [whisper.cpp for Windows x64](https://github.com/ggml-org/whisper.cpp/releases/tag/b5127) |
| `engines/llama/llama-server.exe` | [llama.cpp for Windows x64 with Vulkan](https://github.com/ggml-org/llama.cpp/releases/tag/b12289) |
| `models/ggml-small-q5_1.bin` | [Quantized multilingual Whisper Small](https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin) |
| `models/qwen2.5-3b-instruct-q4_k_m.gguf` | [Qwen2.5 3B Instruct Q4_K_M](https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/blob/main/qwen2.5-3b-instruct-q4_k_m.gguf) |

This setup uses Whisper on the CPU and llama.cpp with Vulkan. In the CLI, use `--cpu` to run both stages on the CPU. Override the default paths with `--whisper`, `--llama`, `--whisper-model` or `--llama-model`.

On Linux, install the native executables at `engines/whisper/Release/whisper-cli` and `engines/llama/llama-server`. Use the same model paths.

## Bring your own model

Lovelace uses two local models. Choose a multilingual Whisper `.bin` for English and Brazilian Portuguese transcription and an instruction-tuned GGUF for summarization. The desktop app reads the default paths above. The CLI accepts custom paths on both `record` and `process`.

```sh
lovelace record --language pt --whisper-model models/ggml-base-q5_1.bin --llama-model models/qwen2.5-1.5b-instruct-q4_k_m.gguf
```

Start with Whisper Small and Qwen2.5 3B Instruct. Try [Whisper Base or Medium](https://huggingface.co/ggerganov/whisper.cpp/tree/main) if you want to compare transcription time and quality, and [Qwen2.5 1.5B](https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF) or [Qwen2.5 7B](https://huggingface.co/Qwen/Qwen2.5-7B-Instruct-GGUF) for smaller or larger summarization models. Both alternatives are published under Apache 2.0. The default [Qwen2.5 3B](https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF) is published under the Qwen Research license.

`process` reuses `whisper.json` when present and keeps the existing transcript Markdown. Use a new recording to test a different transcription model. To compare summary models on the same transcript, run `process` on a copy of the session with a different `--llama-model`. Each run replaces the summary in its session folder.

## Desktop

Run this from the project root in PowerShell.

```powershell
cargo run --release --features desktop --bin lovelace-desktop
```

On Linux, use `cargo +stable run --release --features desktop --bin lovelace-desktop`.

- **Recorder**. The interface is in English. Under **Spoken language**, choose **Portuguese**, **English** or **Auto-detect** for the transcript and summary. Select **Record** to capture from the default microphone and see the elapsed time and input level. **Stop** saves the WAV file and starts processing in the background.
- **History**. The dashboard shows the three most recent sessions. Open **History** for the full list, including sessions recorded with the CLI. Each status shows whether the audio, transcript or summary is ready.
- **Documents and audio**. Open a session to switch between **Summary** and **Transcript**, copy the original Markdown or play the WAV file. Select a timestamp in the summary to open its transcript segment and seek to that point in the audio.
- **Reprocessing**. Select **Process session** to resume processing a session that has no summary. While a recording or processing job is active, the header shows its status in the history, documents and audio views.

## CLI

```powershell
cargo run --release -- microphones
cargo run --release -- record --microphone 0 --audio-only
cargo run --release -- record --language en
cargo run --release -- process ".\sessoes\SESSION-NAME"
```

On Linux, use `cargo +stable run` and pass `./sessoes/SESSION-NAME` to `process`. Replace `SESSION-NAME` with the session folder shown in the terminal. Press Enter to stop recording. The CLI uses the default input unless you pass `--microphone`. Use `--audio-only` to save the WAV file for later processing.

Use `--language pt` for Brazilian Portuguese, `--language en` for English or `--language auto` to detect the main spoken language. Portuguese is the default for new recordings. The original Portuguese commands and flags still work as aliases.

## Regression tests

Run the suite from the project root on Windows.

```powershell
cargo test --locked --release --features desktop
```

On Linux, use `cargo +stable test --locked --release --features desktop` with the build dependencies listed in the workflow. The suite covers CLI commands, session processing and retries, summary references, and the desktop backend for history and Markdown. It uses temporary sessions with generated WAV files and fixed Whisper results, so it runs without a microphone or models. The build workflow runs the suite on both platforms before packaging.

## Pipeline and storage

- CPAL captures audio at the input sample rate and Hound writes a 16-bit mono WAV file. [audio.rs](src/audio.rs)
- Whisper returns timestamped segments that Lovelace renders as Markdown. Reprocessing reuses `whisper.json` and any existing transcript Markdown, then regenerates the summary. [pipeline.rs](src/pipeline.rs)
- Lovelace runs `llama-server` on `127.0.0.1` and summarizes the transcript in batches. An idea revisited in different batches can appear in more than one note. [synthesis.rs](src/synthesis.rs)
- The desktop app exposes Tauri commands to the HTML, CSS and JavaScript frontend. The CLI and desktop app share the same pipeline. [desktop.rs](src/bin/desktop.rs) [Frontend](ui/)

Sessions are stored in `sessoes/sessao-<timestamp>-<suffix>/`.

| File | Contents |
| --- | --- |
| `audio.wav` | Original recording |
| `whisper.json` | Detected or selected language and timestamped speech segments |
| `transcricao.md` | Transcript with segment anchors |
| `sintese.md` | Summary notes with links to the transcript |

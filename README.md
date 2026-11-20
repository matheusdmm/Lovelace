# Lovelace
[![Build binaries](https://github.com/matheusdmm/Lovelace/actions/workflows/build.yml/badge.svg)](https://github.com/matheusdmm/Lovelace/actions/workflows/build.yml)

Lovelace is an idea recorder for Windows and Linux that produces transcripts and summaries in Markdown. Each summary note links to the transcript segments that support it. Lovelace also generates timestamped SRT subtitles from video and audio files, including finished OBS Studio recordings. Media processing and inference stay on your machine.

The [Lovelace website](website/) includes downloads, an audit guide, and instructions for building and running the app. See the [website README](website/README.md) for the Coolify configuration, Docker commands, and release link settings.

## Binaries

The [Build binaries workflow](.github/workflows/build.yml) runs on pushes and pull requests to `main` that change Rust code, tests, dependencies, build configuration, or packaged desktop assets. Changes limited to `website/` or documentation do not trigger a build. You can also run it manually from **Actions**. Downloads are available under **Artifacts** for each run.

Pushing a version tag such as `v1.0.0` builds both platforms and publishes a [GitHub Release](https://github.com/matheusdmm/Lovelace/releases) with the Windows and Linux archives after all tests and builds pass.

| x64 artifact | Contents |
| --- | --- |
| `lovelace-windows-x64.zip` | `lovelace.exe`, `lovelace-desktop.exe`, `WebView2Loader.dll` and `lovelace_captions.py` |
| `lovelace-linux-x64.tar.gz` | `lovelace`, `lovelace-desktop` and `lovelace_captions.py`, built on Ubuntu 24.04 |

The Windows desktop app requires Microsoft Edge WebView2 Runtime. On Linux, the binaries use ALSA and the desktop app also requires WebKitGTK 4.1. Extract the archive and open the desktop app, then configure engines and models under **Settings → Models and engines**. For CLI use, install the engines and models at the paths below or pass custom paths.

## Requirements

- On Windows, build with Rust 1.90 or later, the `stable-x86_64-pc-windows-gnu` toolchain and x64 MinGW-w64 (`gcc.exe` on `PATH`).
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

Lovelace uses a multilingual Whisper `.bin` for transcription and captions, and an instruction-tuned GGUF for summarization. In the desktop app, use **Settings → Models and engines → Browse…** to select models or engine executables from another folder. Selections are saved immediately and apply to recordings, retries, restarts and captions. Without a saved selection, the desktop app uses the default paths above. The CLI accepts custom model paths on `record` and `process`. Use `--whisper-model` to select a custom model for `captions`.

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

Choose **Settings → Appearance → Theme → Loveless**, then **Save settings**, for a dark plum and pink theme inspired by My Bloody Valentine's *Loveless*. The theme also uses dark window controls and is restored when the app reopens.

On first launch, open **Settings → Models and engines**. Choose a Whisper size (Tiny, Base, Small, Medium or Large v3 Turbo), a summary model (Qwen2.5 1.5B or 3B), and download the whisper.cpp and llama.cpp engines. **Download & use** verifies each download against its pinned SHA-256, extracts engine libraries and activates it automatically. **Browse…** selects an existing executable and preserves its surrounding libraries.

Under **llama.cpp · engine**, choose a backend matching your hardware. Downloads are filtered to the operating system and architecture of the running app. The distributed Lovelace binaries are Windows and Linux x64; the engine catalog also includes ARM64 variants for native ARM64 builds.

| Backend | Hardware and requirements |
| --- | --- |
| CPU | Processor only; no GPU runtime required |
| Vulkan | NVIDIA, AMD or Intel GPUs with a compatible Vulkan driver |
| CUDA | NVIDIA GPUs supported by the selected CUDA version, with a compatible NVIDIA driver |
| ROCm | AMD GPUs supported by ROCm 10.0, with a compatible AMD driver; x64 |
| SYCL | Intel GPUs, including supported Arc and integrated GPUs; x64. Linux also needs a compatible oneAPI/Level Zero runtime |
| OpenVINO | Supported Intel hardware; GPU/NPU use needs matching drivers; x64 |
| OpenCL Adreno | Qualcomm Adreno with an OpenCL driver; Windows ARM64 |

CUDA choices include their official companion runtime package. Settings shows the combined download size, verifies every archive and installs runtime libraries beside `llama-server` before activating it. On Windows x64, CUDA 12.4 and 13.5 builds are offered; Linux x64 offers CUDA 12.8 and 13.5. ARM64 offers CUDA 13.5. GPU and driver support depend on the selected backend; choose CPU when GPU acceleration is unavailable. The Linux engine packages are upstream Ubuntu builds and may require additional system libraries on other distributions. Each backend is installed separately, so you can switch between downloaded engines using **Use model / engine**.

Downloads are stored under the application's per-user data directory, shown in Settings. A progress indicator reports bytes transferred, with cancellation and retry after failure. Partial downloads are never activated. **Delete download…** removes a managed download after confirmation; external files are preserved. **Use default path** clears a selection. Model choices persist independently of **Save settings**. Finish or cancel a download before closing the app or starting a job; selections and deletion are blocked while a job is running. The CLI and OBS continue to use their own paths and flags. FFmpeg remains a separate requirement for captions.

- **Recorder**. The interface is in English. Under **Spoken language**, choose **Portuguese**, **English** or **Auto-detect** for the transcript and summary. Select **Record** to capture from the default microphone and see the elapsed time and input level. **Stop** saves the WAV file and starts processing in the background.
- **History**. The dashboard shows the three most recent sessions. Open **History** for the full list, including sessions recorded with the CLI. Each status shows whether the audio, transcript or summary is ready.
- **Documents and audio**. Open a session to switch between **Summary** and **Transcript**, copy the displayed Markdown or play the WAV file. The transcript keeps the full speech in paragraphs without segment labels or timestamps. Paragraphs follow sentence endings and pauses; the speech is not rewritten or summarized. Older segment-based transcripts also use this readable view without modifying the saved originals. Select a timestamp in the summary to open the corresponding transcript paragraph and seek to that point in the audio.
- **Reprocessing**. Select **Resume processing** to reuse a completed transcription and finish a session without a summary. **Restart transcription…** starts again from the audio, including when that session is currently transcribing or generating its summary. It stops and waits for the previous engine, backs up existing results to a `restart-backup-*` folder in the session, and regenerates both documents. The audio is preserved.
- **Transcription log**. This sidebar section appears after a job starts and remains available until the app closes. It shows the current stage, elapsed time, activity and expandable Whisper output, with resume/restart controls for recording sessions. The percentage represents stages and stays fixed while a stage runs. The Recorder stays focused on recording controls and recent sessions. Whisper output is also saved as `whisper.log` in the session folder.

## Captions

Install [FFmpeg](https://ffmpeg.org/download.html) on `PATH` and set up the Whisper engine and model from [Requirements](#requirements). The caption pipeline uses FFmpeg to extract audio and Whisper to transcribe it with timestamps. If Lovelace or OBS was open when you updated `PATH`, restart it.

By default, Lovelace saves the SRT next to the input, so `recording.mkv` produces `recording.srt`. It preserves the original media and any existing SRT. Move or rename the old SRT to regenerate it, or choose another destination from the CLI.

### From the desktop app

1. Open **Create captions** on the dashboard.
2. Drop a video or audio file into the window, or paste its path into **Media file**.
3. Choose **Portuguese**, **English**, or **Auto-detect** under **Spoken language**. **Audio track** defaults to 1 and follows the order of the audio tracks in the file.
4. Select **Generate SRT**. The app displays the subtitle path when it finishes.

### From OBS Studio

1. In OBS Studio 29 or later, open **Tools → Scripts**. If your OBS build requires it, configure a compatible Python installation under **Python Settings**. [OBS scripting guide](https://docs.obsproject.com/scripting)
2. Add [lovelace_captions.py](obs/lovelace_captions.py) and set **Lovelace CLI** to `lovelace.exe` on Windows or `lovelace` on Linux.
3. If you built Lovelace from source and selected the executable under `target/release`, set **Folder containing engines and models** to the repository root. If you extracted a release and installed the engines and models beside the executable, leave this field empty.
4. Select the spoken language and audio track, and leave **Generate captions after recording** enabled. Stop a recording to start caption generation in the background. Check **Script Log** for the result.

If OBS splits a recording across multiple files, the script captions the last file reported by OBS. Live captions and direct capture from an OBS audio source are outside the current workflow.

### From the command line

```sh
lovelace captions "recording.mkv" --language pt --audio-track 1
lovelace captions "interview.wav" --language auto --output "interview-captions.srt"
```

Run the CLI from the folder containing `engines/` and `models/` when using the default paths. Use `--ffmpeg` to point to an FFmpeg executable outside `PATH`.

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

On Linux, use `cargo +stable test --locked --release --features desktop` with the build dependencies listed in the workflow. The suite covers CLI commands, session processing and retries, summary references, the desktop backend for history and Markdown, SRT timing, and preservation of existing subtitles. It uses temporary sessions with generated WAV files and fixed Whisper results, so it runs without a microphone or models. The build workflow runs the suite on both platforms before packaging.

## Pipeline and storage

- CPAL captures audio at the input sample rate and Hound writes a 16-bit mono WAV file. [audio.rs](src/audio.rs)
- Whisper returns timestamped segments that Lovelace joins into full-text Markdown paragraphs. Timestamps and synthesis appear in the summary. Reprocessing reuses `whisper.json` and any existing transcript Markdown, then regenerates the summary. [pipeline.rs](src/pipeline.rs), [transcript.rs](src/transcript.rs)
- Lovelace runs `llama-server` on `127.0.0.1` and summarizes the transcript in batches. An idea revisited in different batches can appear in more than one note. [synthesis.rs](src/synthesis.rs)
- The desktop app exposes Tauri commands to the HTML, CSS and JavaScript frontend. The CLI and desktop app share the same pipeline. [desktop.rs](src/bin/desktop.rs) [Frontend](ui/)

Sessions are stored in `sessoes/sessao-<timestamp>-<suffix>/`.

| File | Contents |
| --- | --- |
| `audio.wav` | Original recording |
| `whisper.json` | Detected or selected language and timestamped speech segments |
| `transcricao.md` | Full transcript in paragraphs, without timestamps or segment labels |
| `sintese.md` | Summary notes with links to the transcript |

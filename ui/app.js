const { invoke, convertFileSrc } = window.__TAURI__.core;
const byId = (id) => document.getElementById(id);

const toolbar = document.querySelector(".toolbar");
const history = byId("history");
const review = byId("review");
const empty = byId("empty");
const article = byId("document");
const pane = byId("reading-pane");
const buttons = [...document.querySelectorAll("[data-document]")];
const record = byId("record");
const language = byId("language");
const retry = byId("retry");
const copy = byId("copy");
const audio = byId("audio");
const play = byId("play");
const seek = byId("seek");
const elapsed = byId("elapsed");
const level = byId("level");
const statusText = byId("status");
const message = byId("message");

const dateFormat = new Intl.DateTimeFormat("pt-BR", { day: "numeric", month: "long" });
const timeFormat = new Intl.DateTimeFormat("pt-BR", { hour: "2-digit", minute: "2-digit" });
const titleFormat = new Intl.DateTimeFormat("pt-BR", {
  day: "numeric", month: "long", year: "numeric", hour: "2-digit", minute: "2-digit",
});

const phases = {
  idle: "Pronto para gravar",
  starting: "Abrindo microfone",
  recording: "Gravando",
  saving: "Salvando áudio",
  preparing: "Preparando sessão",
  transcribing: "Transcrevendo",
  synthesizing: "Gerando síntese",
  done: "Sessão pronta",
  failed: "Sessão interrompida",
};

let sessions = [];
let selected = null;
let opened = null;
let currentDocument = "summary";
let request = 0;
let phase = "idle";
let recordingSince = 0;
let pendingSeek = null;
let pendingCommand = false;
let copyTimeout;

const isBusy = () => !["idle", "done", "failed"].includes(phase);

function showMessage(text = "") {
  message.textContent = text;
  message.hidden = !text;
}

function clock(seconds) {
  if (!Number.isFinite(seconds)) return "00:00";
  const total = Math.max(0, Math.floor(seconds));
  const hours = Math.floor(total / 3600);
  const minutes = String(Math.floor(total / 60) % 60).padStart(2, "0");
  const rest = String(total % 60).padStart(2, "0");
  return hours ? `${hours}:${minutes}:${rest}` : `${minutes}:${rest}`;
}

function renderHistory() {
  const items = document.createDocumentFragment();
  for (const session of sessions) {
    const date = new Date(session.timestamp);
    const button = document.createElement("button");
    button.className = "session";
    button.dataset.id = session.id;
    if (session.id === selected?.id) button.setAttribute("aria-current", "true");
    const name = document.createElement("strong");
    name.textContent = dateFormat.format(date);
    const detail = document.createElement("small");
    const state = session.has_summary ? "Síntese pronta"
      : session.has_transcript ? "Transcrita" : "Áudio salvo";
    detail.textContent = `${timeFormat.format(date)} · ${state}`;
    button.append(name, detail);
    items.append(button);
  }
  if (sessions.length === 0) {
    const label = document.createElement("p");
    label.textContent = "Nenhuma sessão";
    items.append(label);
  }
  history.replaceChildren(items);
}

async function refreshHistory(preferred) {
  sessions = await invoke("history");
  const next = sessions.find(({ id }) => id === (preferred ?? selected?.id)) ?? sessions[0];
  renderHistory();
  if (next && (!opened || preferred || next.id !== selected?.id)) await openSession(next);
}

async function openSession(session) {
  const reading = ++request;
  selected = session;
  opened = null;
  for (const button of history.querySelectorAll(".session")) {
    if (button.dataset.id === session.id) button.setAttribute("aria-current", "true");
    else button.removeAttribute("aria-current");
  }
  pendingSeek = null;
  audio.removeAttribute("src");
  audio.load();
  renderPlayer();
  byId("session-title").textContent = titleFormat.format(new Date(session.timestamp));
  empty.hidden = true;
  review.hidden = false;
  byId("player").hidden = true;
  showDocument("summary");
  try {
    const data = await invoke("review", { id: session.id });
    if (reading !== request) return;
    opened = data;
    audio.src = convertFileSrc(data.audio_path);
    byId("player").hidden = false;
    showDocument(data.summary || !data.transcript ? "summary" : "transcript");
  } catch (error) {
    if (reading !== request) return;
    article.textContent = `Não foi possível abrir a sessão. ${error}`;
  }
}

function showDocument(kind) {
  currentDocument = kind;
  clearTimeout(copyTimeout);
  copy.textContent = "Copiar Markdown";
  copy.disabled = !opened?.[kind];
  for (const button of buttons) {
    button.setAttribute("aria-pressed", String(button.dataset.document === kind));
    button.disabled = !opened;
  }
  retry.hidden = !opened || Boolean(opened.summary);
  retry.disabled = isBusy() || pendingCommand;
  const data = opened?.[kind];
  if (data) {
    // O Rust renderiza o Markdown e restringe HTML e links antes de enviá-lo.
    article.innerHTML = data.html;
    if (kind === "transcript") {
      for (const heading of article.querySelectorAll("h3")) {
        const match = /^Trecho (\d+)$/.exec(heading.textContent);
        if (match) heading.id = `trecho-${match[1]}`;
      }
    }
  } else {
    article.innerHTML = "";
    const label = document.createElement("p");
    label.className = "unavailable";
    label.textContent = opened
      ? `Esta sessão ainda está sem ${kind === "summary" ? "síntese" : "transcrição"}.`
      : "Abrindo sessão…";
    article.append(label);
  }
  pane.scrollTop = 0;
}

function renderPlayer() {
  const ready = Number.isFinite(audio.duration) && audio.duration > 0;
  const playing = ready && !audio.paused && !audio.ended;
  play.disabled = !ready;
  play.dataset.playing = String(playing);
  play.setAttribute("aria-label", playing ? "Pausar" : "Reproduzir");
  seek.disabled = !ready;
  seek.max = ready ? audio.duration : 1;
  seek.value = ready ? audio.currentTime : 0;
  seek.setAttribute("aria-valuetext", clock(audio.currentTime));
  byId("current-time").textContent = clock(audio.currentTime);
  byId("duration").textContent = clock(audio.duration);
}

function seekAudio(seconds) {
  if (Number.isFinite(audio.duration)) {
    audio.currentTime = Math.min(seconds, audio.duration);
    renderPlayer();
  } else {
    pendingSeek = seconds;
  }
}

function jumpToExcerpt(event) {
  const link = event.target.closest("a");
  if (!link) return;
  event.preventDefault();
  const target = /^transcricao\.md#(trecho-\d+)$/.exec(link.getAttribute("href"));
  if (!target || !opened?.transcript) return;
  const time = /^(\d+):(\d{2}):(\d{2}(?:\.\d+)?)$/.exec(link.textContent.trim());
  showDocument("transcript");
  const heading = document.getElementById(target[1]);
  if (heading) {
    heading.classList.add("highlight");
    heading.tabIndex = -1;
    heading.focus({ preventScroll: true });
    heading.scrollIntoView({ block: "start" });
  }
  if (time) seekAudio(Number(time[1]) * 3600 + Number(time[2]) * 60 + Number(time[3]));
}

async function copyMarkdown() {
  const data = opened?.[currentDocument];
  if (!data) return;
  try {
    await navigator.clipboard.writeText(data.markdown);
  } catch {
    const field = document.createElement("textarea");
    field.className = "clipboard-source";
    field.value = data.markdown;
    document.body.append(field);
    field.select();
    const copied = document.execCommand("copy");
    field.remove();
    copy.focus();
    if (!copied) {
      showMessage("Não foi possível copiar o Markdown.");
      return;
    }
  }
  if (opened?.[currentDocument] === data) {
    copy.textContent = "Copiado";
    clearTimeout(copyTimeout);
    copyTimeout = setTimeout(() => { copy.textContent = "Copiar Markdown"; }, 1600);
  }
}

function renderStatus(state) {
  const recording = state.phase === "recording";
  if (recording && phase !== state.phase) recordingSince = performance.now();
  phase = state.phase;
  toolbar.dataset.phase = phase;
  if (statusText.textContent !== phases[phase]) statusText.textContent = phases[phase];
  record.lastChild.textContent = recording ? "Encerrar" : "Gravar";
  record.dataset.recording = String(recording);
  record.disabled = pendingCommand || (isBusy() && !recording);
  language.disabled = pendingCommand || isBusy();
  retry.disabled = pendingCommand || isBusy();
  elapsed.hidden = !recording;
  level.hidden = !recording;
  if (recording) {
    elapsed.textContent = clock((performance.now() - recordingSince) / 1000);
    level.value = state.level;
  }
  if (!selected) {
    byId("empty-title").textContent = isBusy() ? phases[phase] : "Sua primeira gravação";
    byId("empty-description").textContent = isBusy()
      ? "A sessão aparece no histórico assim que o processamento terminar."
      : "Grave uma ideia para ler a síntese e voltar aos trechos da conversa.";
  }
}

let pollTimeout;
let pollGeneration = 0;
async function poll(force = false) {
  clearTimeout(pollTimeout);
  const generation = ++pollGeneration;
  try {
    const previous = phase;
    const state = await invoke("status");
    if (generation !== pollGeneration) return;
    renderStatus(state);
    if (isBusy()) {
      pollTimeout = setTimeout(poll, phase === "recording" ? 120 : 420);
    } else if (force || previous !== phase || !selected) {
      if (state.error) showMessage(state.error);
      await refreshHistory(state.session);
    }
  } catch (error) {
    showMessage(String(error));
  }
}

async function command(name, args) {
  if (pendingCommand) return;
  pendingCommand = true;
  pollGeneration++;
  record.disabled = true;
  retry.disabled = true;
  language.disabled = true;
  clearTimeout(pollTimeout);
  showMessage();
  let submitted = false;
  try {
    await invoke(name, args);
    submitted = true;
  } catch (error) {
    showMessage(String(error));
  } finally {
    pendingCommand = false;
    await poll(submitted);
  }
}

history.addEventListener("click", (event) => {
  const button = event.target.closest(".session");
  if (!button) return;
  const session = sessions.find(({ id }) => id === button.dataset.id);
  if (session) void openSession(session);
});
record.addEventListener("click", () => {
  if (phase === "recording") void command("stop_recording");
  else if (!isBusy()) void command("start_recording", { language: language.value });
});
retry.addEventListener("click", () => {
  if (selected && !isBusy()) void command("retry", { id: selected.id, language: language.value });
});
for (const button of buttons) {
  button.addEventListener("click", () => showDocument(button.dataset.document));
}
article.addEventListener("click", jumpToExcerpt);
copy.addEventListener("click", copyMarkdown);
play.addEventListener("click", async () => {
  if (!audio.paused) audio.pause();
  else {
    try {
      await audio.play();
    } catch (error) {
      if (error.name !== "AbortError") showMessage("Não foi possível reproduzir o áudio.");
    }
  }
});
seek.addEventListener("input", () => seekAudio(Number(seek.value)));
for (const name of ["durationchange", "timeupdate", "play", "pause", "ended", "emptied"]) {
  audio.addEventListener(name, renderPlayer);
}
audio.addEventListener("loadedmetadata", () => {
  if (pendingSeek !== null) seekAudio(pendingSeek);
  pendingSeek = null;
  renderPlayer();
});
audio.addEventListener("error", () => {
  if (audio.hasAttribute("src")) showMessage("Não foi possível carregar o áudio desta sessão.");
});

void window.__TAURI__.event.listen("session-active", () => {
  showMessage(phase === "recording"
    ? "Encerre a gravação antes de fechar o Lovelace."
    : "Aguarde a sessão terminar antes de fechar o Lovelace.");
}).catch((error) => console.error(error));
void poll();

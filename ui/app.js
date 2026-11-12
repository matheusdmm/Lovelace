const { invoke, convertFileSrc } = window.__TAURI__.core;
const byId = (id) => document.getElementById(id);

const workspace = document.querySelector(".workspace");
const toolbar = document.querySelector(".toolbar");
const history = byId("history");
const sessionCount = byId("session-count");
const sessionCountLabel = byId("session-count-label");
const sessionState = byId("session-state");
const previewTitle = byId("preview-title");
const previewDetail = byId("preview-detail");
const audioSession = byId("audio-session");
const back = byId("back");
const activeJob = byId("active-job");
const review = byId("review");
const empty = byId("empty");
const article = byId("document");
const pane = byId("reading-pane");
const buttons = [...document.querySelectorAll("[data-document]")];
const record = byId("record");
const recordLabel = byId("record-label");
const language = byId("language");
const retry = byId("retry");
const copy = byId("copy");
const audio = byId("audio");
const play = byId("play");
const seek = byId("seek");
const elapsed = byId("elapsed");
const level = byId("level");
const signalChart = byId("signal-chart");
const signalContext = signalChart.getContext("2d");
const signalSamples = new Float32Array(48);
const message = byId("message");

const dateFormat = new Intl.DateTimeFormat("en-US", { day: "numeric", month: "long" });
const timeFormat = new Intl.DateTimeFormat("en-US", { hour: "2-digit", minute: "2-digit" });
const titleFormat = new Intl.DateTimeFormat("en-US", {
  day: "numeric", month: "long", year: "numeric", hour: "2-digit", minute: "2-digit",
});

const phases = {
  idle: "Ready to record",
  starting: "Opening microphone",
  recording: "Recording",
  saving: "Saving audio",
  preparing: "Preparing session",
  transcribing: "Transcribing",
  synthesizing: "Generating summary",
  done: "Session ready",
  failed: "Session interrupted",
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
let dashboardOpener;

const isBusy = () => !["idle", "done", "failed"].includes(phase);

function showView(view, opener) {
  const previous = workspace.dataset.view;
  if (previous === view) return;
  if (previous === "dashboard") dashboardOpener = opener;
  workspace.dataset.view = view;
  workspace.scrollTop = 0;
  back.hidden = view === "dashboard";
  activeJob.hidden = !isBusy() || view === "dashboard" || view === "record";

  const heading = {
    dashboard: dashboardOpener?.isConnected ? dashboardOpener : byId("studio-heading"),
    record: byId("studio-heading"),
    history: byId("history-heading"),
    documents: selected ? byId("session-title") : byId("empty-title"),
    audio: byId("audio-heading"),
  }[view];
  heading.focus({ preventScroll: true });
}

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

function sessionStatus(session) {
  if (session.has_summary) return "Summary ready";
  if (session.has_transcript) return "Transcribed";
  return "Audio saved";
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
    detail.textContent = `${timeFormat.format(date)} · ${sessionStatus(session)}`;
    button.append(name, detail);
    items.append(button);
  }
  if (sessions.length === 0) {
    const label = document.createElement("p");
    label.textContent = "No sessions";
    items.append(label);
  }
  history.replaceChildren(items);
  sessionCount.textContent = sessions.length;
  sessionCountLabel.textContent = sessions.length === 1 ? "session" : "sessions";
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
  const sessionDate = titleFormat.format(new Date(session.timestamp));
  sessionState.textContent = sessionStatus(session);
  byId("session-title").textContent = sessionDate;
  audioSession.textContent = sessionDate;
  audioSession.hidden = false;
  previewTitle.textContent = sessionStatus(session);
  previewDetail.textContent = sessionDate;
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
    if (data.summary || data.transcript) {
      previewTitle.textContent = data.summary ? "Session summary" : "Session transcript";
      const excerpt = article.querySelector(data.summary ? "p, li" : "h3 ~ p:not(:has(strong))");
      if (excerpt?.textContent.trim()) previewDetail.textContent = excerpt.textContent.trim();
    }
  } catch (error) {
    if (reading !== request) return;
    article.textContent = `Could not open the session. ${error}`;
  }
}

function showDocument(kind) {
  currentDocument = kind;
  clearTimeout(copyTimeout);
  copy.textContent = "Copy Markdown";
  copy.disabled = !opened?.[kind];
  for (const button of buttons) {
    button.setAttribute("aria-pressed", String(button.dataset.document === kind));
    button.disabled = !opened;
  }
  retry.hidden = !opened || Boolean(opened.summary);
  retry.disabled = isBusy() || pendingCommand;
  const data = opened?.[kind];
  if (data) {
    // The Rust backend renders Markdown and restricts HTML and links.
    article.innerHTML = data.html;
    if (kind === "transcript") {
      for (const heading of article.querySelectorAll("h3")) {
        const match = /^(?:Segment|Trecho) (\d+)$/.exec(heading.textContent);
        if (match) heading.id = `segment-${match[1]}`;
      }
    }
  } else {
    article.innerHTML = "";
    const label = document.createElement("p");
    label.className = "unavailable";
    label.textContent = opened
      ? `This session does not have a ${kind} yet.`
      : "Opening session…";
    article.append(label);
  }
  pane.scrollTop = 0;
}

function renderPlayer() {
  const ready = Number.isFinite(audio.duration) && audio.duration > 0;
  const playing = ready && !audio.paused && !audio.ended;
  play.disabled = !ready;
  play.dataset.playing = String(playing);
  play.setAttribute("aria-label", playing ? "Pause" : "Play");
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
  const target = /^transcricao\.md#(?:segment|trecho)-(\d+)$/.exec(link.getAttribute("href"));
  if (!target || !opened?.transcript) return;
  const time = /^(\d+):(\d{2}):(\d{2}(?:\.\d+)?)$/.exec(link.textContent.trim());
  showDocument("transcript");
  const heading = document.getElementById(`segment-${target[1]}`);
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
      showMessage("Could not copy Markdown.");
      return;
    }
  }
  if (opened?.[currentDocument] === data) {
    copy.textContent = "Copied";
    clearTimeout(copyTimeout);
    copyTimeout = setTimeout(() => { copy.textContent = "Copy Markdown"; }, 1600);
  }
}

function renderSignal(recording, sample) {
  level.value = recording ? sample : 0;
  if (!signalContext) return;
  const { width, height } = signalChart;
  signalContext.clearRect(0, 0, width, height);
  if (!recording) {
    signalSamples.fill(0);
    return;
  }
  signalSamples.copyWithin(0, 1);
  signalSamples[signalSamples.length - 1] = level.value;
  const step = width / signalSamples.length;
  signalContext.fillStyle = "#fdf0d5";
  for (const [index, value] of signalSamples.entries()) {
    const bar = Math.max(1, Math.sqrt(value) * height);
    signalContext.fillRect(index * step, height - bar, step * .68, bar);
  }
}

function renderStatus(state) {
  const recording = state.phase === "recording";
  const changed = phase !== state.phase;
  if (recording && changed) recordingSince = performance.now();
  phase = state.phase;
  toolbar.dataset.phase = phase;
  const view = workspace.dataset.view;
  activeJob.hidden = !isBusy() || view === "dashboard" || view === "record";
  activeJob.textContent = phases[phase];
  recordLabel.textContent = recording ? "Stop" : "Record";
  record.dataset.recording = String(recording);
  record.disabled = pendingCommand || (isBusy() && !recording);
  language.disabled = pendingCommand || isBusy();
  retry.disabled = pendingCommand || isBusy();
  if (phase === "starting") elapsed.textContent = "00:00";
  if (recording) {
    elapsed.textContent = clock((performance.now() - recordingSince) / 1000);
  }
  renderSignal(recording, state.level);
  if (!selected && changed) {
    byId("empty-title").textContent = isBusy() ? phases[phase] : "Your first recording";
    byId("empty-description").textContent = recording
      ? "Select Stop to save the audio and start processing."
      : isBusy() ? "The session will appear in History when processing finishes."
        : "The transcript and summary will appear here after processing.";
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

for (const button of document.querySelectorAll("[data-view-target]")) {
  button.addEventListener("click", () => showView(button.dataset.viewTarget, button));
}
history.addEventListener("click", (event) => {
  const button = event.target.closest(".session");
  if (!button) return;
  const session = sessions.find(({ id }) => id === button.dataset.id);
  if (session) {
    if (session.id !== selected?.id || !opened) void openSession(session);
    showView("documents", button);
  }
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
      if (error.name !== "AbortError") showMessage("Could not play the audio.");
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
  if (audio.hasAttribute("src")) showMessage("Could not load this session’s audio.");
});

void window.__TAURI__.event.listen("session-active", () => {
  showMessage(phase === "recording"
    ? "Stop recording before closing Lovelace."
    : "Wait for the session to finish before closing Lovelace.");
}).catch((error) => console.error(error));
void poll();

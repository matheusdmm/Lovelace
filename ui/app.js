const { invoke, convertFileSrc } = window.__TAURI__.core;
const byId = (id) => document.getElementById(id);
const textNodes = new Map(
  Array.from(document.querySelectorAll("[data-text]"), (node) => [node.dataset.text, node]),
);

const workspace = document.querySelector(".workspace");
const viewHeading = textNodes.get("view-heading");
const viewPanels = [...workspace.querySelectorAll("[data-panel]")];
const navigation = [...document.querySelectorAll(".nav-item")];
const history = byId("history");
const recent = byId("recent");
const sessionMenu = byId("session-menu");
const menuItems = [...sessionMenu.querySelectorAll("[data-action]")];
const deleteDialog = byId("delete-session");
const activeJob = byId("active-job");
const review = byId("review");
const empty = byId("empty");
const article = byId("document");
const pane = byId("reading-pane");
const buttons = [...document.querySelectorAll("[data-document]")];
const record = byId("record");
const language = byId("language");
const audioInput = byId("audio-input");
const refreshInputs = byId("refresh-audio-inputs");
const processingStatus = byId("processing-status");
const processingState = byId("processing-state");
const processingDetail = byId("processing-detail");
const processingPercent = byId("processing-percent");
const processingProgress = byId("processing-progress");
const processingProgressBar = byId("processing-progress-bar");
const processingLog = byId("processing-log");
const captionMedia = byId("caption-media");
const captionBrowse = byId("pick-caption-media");
const captionLanguage = byId("caption-language");
const captionTrack = byId("caption-track");
const captionSubmit = byId("generate-captions");
const captionStatus = byId("caption-status");
const captionOutput = byId("caption-output");
const retry = byId("retry");
const restart = byId("restart");
const restartDialog = byId("restart-session");
const logResume = byId("log-resume");
const logRestart = byId("log-restart");
const copy = byId("copy");
const audio = byId("audio");
const play = byId("play");
const seek = byId("seek");
const level = byId("level");
const signalChart = byId("signal-chart");
const signalContext = signalChart?.getContext("2d");
const signalSamples = new Float32Array(48);
const message = byId("message");
const settingsForm = byId("settings-form");
const settingsFields = [...settingsForm.querySelectorAll("fieldset")];
const appearanceSetting = byId("setting-appearance");
const backdropSetting = byId("setting-backdrop");
const languageSetting = byId("setting-language");
const settingsSave = byId("save-settings");
const settingsStatus = byId("settings-status");
const modelControls = byId("model-controls");
const modelsStatus = byId("models-status");
const deleteModelDialog = byId("delete-model");
const modelGroups = new Map();
let modelSnapshot;
let modelActionPending = false;
let modelRefreshTimer;
let modelRefreshGeneration = 0;
let pendingModelDeletion;

const dateFormat = new Intl.DateTimeFormat("en-US", { day: "numeric", month: "long" });
const sessionDateFormat = new Intl.DateTimeFormat("en-US", { day: "numeric", month: "long", year: "numeric" });
const timeFormat = new Intl.DateTimeFormat("en-US", { hour: "2-digit", minute: "2-digit" });
const logTimeFormat = new Intl.DateTimeFormat("en-US", {
  hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false,
});
const titleFormat = new Intl.DateTimeFormat("en-US", {
  day: "numeric", month: "long", year: "numeric", hour: "2-digit", minute: "2-digit",
});

const phases = {
  idle: "Ready to record",
  starting: "Opening audio input",
  recording: "Recording",
  saving: "Saving audio",
  preparing: "Preparing session",
  extracting: "Extracting audio",
  transcribing: "Transcribing",
  synthesizing: "Generating summary",
  done: "Session ready",
  failed: "Session interrupted",
};
const phaseDetails = {
  idle: "Waiting for a recording.",
  starting: "Connecting to the selected audio input.",
  recording: "Capturing audio from the selected input.",
  saving: "Closing and saving the WAV recording.",
  preparing: "Preparing the recorded session for processing.",
  extracting: "Extracting and normalizing the audio track.",
  transcribing: "Converting speech into text with the local model.",
  synthesizing: "Creating the digested summary from the transcript.",
  done: "The transcript and summary are ready.",
  failed: "Processing stopped before completion.",
};
const phaseProgress = {
  idle: 0,
  starting: 8,
  recording: 18,
  saving: 28,
  preparing: 38,
  extracting: 52,
  transcribing: 72,
  synthesizing: 90,
  done: 100,
};
const viewTitles = { record: "Recorder", sessions: "Sessions", captions: "Captions", log: "Transcription log", settings: "Settings" };

let sessions = [];
let selected = null;
let opened = null;
let currentDocument = "summary";
let request = 0;
let phase = "idle";
let recordingSince = 0;
let pendingSeek = null;
let pendingCommand = false;
let mutatingSession = false;
let menuOpener = null;
let pendingDeletion = null;
let copyTimeout;
let preferences;
let pickingCaptionMedia = false;
let loadingAudioInputs = false;
let processingProgressValue = 0;
let jobState = { phase: "idle" };
let pendingRestart = null;
let jobStartedAt = 0;

const isBusy = () => !["idle", "done", "failed"].includes(phase);

function renderText(name, text) {
  const node = textNodes.get(name);
  if (node && node.textContent !== text) node.textContent = text;
}

function showView(view) {
  if (workspace.dataset.view === view) return;
  closeSessionMenu();
  workspace.dataset.view = view;
  renderText("view-heading", viewTitles[view]);
  for (const panel of viewPanels) panel.hidden = panel.dataset.panel !== view;
  for (const button of navigation) {
    if (button.dataset.viewTarget === view) {
      button.setAttribute("aria-current", "page");
    } else {
      button.removeAttribute("aria-current");
    }
  }
  updateActiveJob();
  if (view === "settings") void refreshModels();
  viewHeading?.focus({ preventScroll: true });
}

const modelKinds = {
  whisper_model: "Whisper · transcription model",
  llama_model: "llama.cpp · summary model",
  whisper_engine: "whisper.cpp · engine",
  llama_engine: "llama.cpp · engine",
};
const downloadBusy = () => ["downloading", "installing", "cancelling"].includes(modelSnapshot?.download.stage);
const formatBytes = (bytes) => bytes >= 1e9 ? `${(bytes / 1e9).toFixed(2)} GB` : `${(bytes / 1e6).toFixed(1)} MB`;
const assetBytes = (asset) => asset.download_size ?? asset.size;

function setupModelControls() {
  for (const [kind, title] of Object.entries(modelKinds)) {
    const group = document.createElement("div");
    group.className = "model-group";
    const label = document.createElement("label");
    const select = document.createElement("select");
    select.id = `model-${kind}`;
    label.htmlFor = select.id;
    label.textContent = title;
    const meta = document.createElement("p");
    meta.className = "model-meta";
    const requirements = document.createElement("p");
    requirements.className = "model-meta";
    requirements.id = `model-requirements-${kind}`;
    const path = document.createElement("p");
    path.className = "model-path";
    path.id = `model-path-${kind}`;
    select.setAttribute("aria-describedby", `${requirements.id} ${path.id}`);
    const actions = document.createElement("div");
    actions.className = "model-actions";
    const makeButton = (text, handler) => {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "button secondary";
      button.textContent = text;
      button.addEventListener("click", handler);
      actions.append(button);
      return button;
    };
    const install = makeButton("Download & use", () => {
      const asset = modelSnapshot?.catalog.find((asset) => asset.id === select.value);
      if (!asset || install.disabled) return;
      void modelAction(asset.path ? "use_model_path" : "download_model", asset.path ? { kind, path: asset.path } : { id: asset.id });
    });
    const browse = makeButton("Browse…", () => void browseModel(kind));
    const remove = makeButton("Delete download…", () => {
      const asset = modelSnapshot?.catalog.find((asset) => asset.id === select.value);
      if (!asset?.path || remove.disabled) return;
      pendingModelDeletion = asset.id;
      byId("delete-model-description").textContent = `Delete ${asset.label} (${formatBytes(assetBytes(asset))} download) from Lovelace storage? If selected, its selection will return to the default path. You can download it again later.`;
      deleteModelDialog.returnValue = "";
      deleteModelDialog.showModal();
    });
    const reset = makeButton("Use default path", () => void modelAction("use_model_path", { kind, path: null }));
    group.append(label, select, requirements, meta, path, actions);
    modelControls.append(group);
    modelGroups.set(kind, { select, requirements, meta, path, install, browse, remove, reset, signature: "" });
    select.addEventListener("change", renderModels);
  }
}

function renderModels() {
  if (!modelSnapshot) return;
  byId("model-setup-hint").hidden = modelSnapshot.active.every((active) => active.exists);
  const busy = modelActionPending || downloadBusy() || modelSnapshot.job_busy;
  for (const [kind, controls] of modelGroups) {
    const assets = modelSnapshot.catalog.filter((asset) => asset.kind === kind);
    const active = modelSnapshot.active.find((active) => active.kind === kind);
    const signature = JSON.stringify(assets.map((asset) => [asset.id, asset.path]));
    if (signature !== controls.signature) {
      const chosen = controls.select.value;
      controls.select.replaceChildren(...assets.map((asset) => new Option(`${asset.label} · ${formatBytes(assetBytes(asset))}${asset.path ? " · installed" : ""}`, asset.id)));
      const selected = assets.find((asset) => asset.id === chosen) || assets.find((asset) => asset.path === active?.path)
        || assets.find((asset) => ["whisper-small", "qwen-3b"].includes(asset.id)) || assets[0];
      controls.select.value = selected?.id || "";
      controls.signature = signature;
    }
    const asset = assets.find((asset) => asset.id === controls.select.value);
    const inUse = asset?.path && asset.path === active?.path;
    controls.path.textContent = `Selected${active?.exists ? "" : " · missing"}: ${active?.path || "No file selected"}`;
    controls.requirements.textContent = asset?.notes || (kind === "llama_engine" ? "Choose a backend for your graphics hardware, or CPU to run without a GPU." : "");
    controls.requirements.hidden = !controls.requirements.textContent;
    controls.meta.textContent = asset ? `${asset.license} · ${asset.source}` : "No bundled download for this system and architecture. Browse to use a local engine.";
    controls.install.textContent = inUse ? "Selected" : asset?.path ? "Use model / engine" : "Download & use";
    controls.install.disabled = busy || !asset || Boolean(inUse);
    controls.select.disabled = busy || !assets.length;
    controls.browse.disabled = busy;
    controls.reset.disabled = busy;
    controls.remove.disabled = busy || !asset?.path;
  }
  byId("models-directory").textContent = `Download folder: ${modelSnapshot.directory}`;
  const transfer = modelSnapshot.download;
  const hasTransfer = Boolean(transfer.id);
  byId("model-transfer").hidden = !hasTransfer;
  const asset = modelSnapshot.catalog.find((asset) => asset.id === transfer.id);
  const percent = transfer.total ? Math.min(100, Math.floor(transfer.received / transfer.total * 100)) : 0;
  const transferText = {
    downloading: `${asset?.label || "Downloading"}: ${percent}% · ${formatBytes(transfer.received)} / ${formatBytes(transfer.total)}`,
    installing: "Download verified. Installing…",
    cancelling: "Cancelling download…",
    done: "Download installed and selected. Ready to use.",
    cancelled: "Download cancelled. Temporary files removed.",
    failed: transfer.error || "Download failed. Please retry.",
  };
  byId("model-transfer-status").textContent = transferText[transfer.stage] || "";
  byId("model-transfer-progress").value = percent;
  byId("model-transfer-progress").hidden = !downloadBusy();
  byId("cancel-model-download").hidden = !downloadBusy();
  byId("cancel-model-download").disabled = modelActionPending || transfer.stage === "cancelling";
}

async function refreshModels() {
  clearTimeout(modelRefreshTimer);
  const generation = ++modelRefreshGeneration;
  try {
    const snapshot = await invoke("get_models");
    if (generation !== modelRefreshGeneration) return;
    modelSnapshot = snapshot;
    renderModels();
  } catch (error) {
    if (generation === modelRefreshGeneration) modelsStatus.textContent = `Could not load models. ${error}`;
  } finally {
    if (generation === modelRefreshGeneration && (workspace.dataset.view === "settings" || downloadBusy())) modelRefreshTimer = setTimeout(refreshModels, 750);
  }
}

async function modelAction(command, args) {
  if (modelActionPending) return;
  modelActionPending = true;
  modelsStatus.textContent = "";
  renderModels();
  try {
    await invoke(command, args);
    modelsStatus.textContent = command === "download_model" ? "Download started. You can cancel it below."
      : command === "delete_model" ? "Download deleted." : command === "cancel_model_download" ? "Cancelling…" : "Selection saved.";
  } catch (error) {
    modelsStatus.textContent = String(error);
  } finally {
    modelActionPending = false;
    await refreshModels();
  }
}

async function browseModel(kind) {
  if (modelActionPending || downloadBusy() || modelSnapshot?.job_busy) return;
  modelActionPending = true;
  renderModels();
  let path;
  try { path = await invoke("pick_model_file", { kind }); }
  catch (error) { modelsStatus.textContent = String(error); }
  finally { modelActionPending = false; renderModels(); }
  if (path) await modelAction("use_model_path", { kind, path });
}

function updateActiveJob(view) {
  if (!activeJob) return;
  if (view) {
    activeJob.dataset.viewTarget = view;
    activeJob.textContent = phases[phase];
  }
  activeJob.hidden = !isBusy() || workspace.dataset.view === activeJob.dataset.viewTarget;
}

function showMessage(text = "") {
  message.textContent = text;
  message.hidden = !text;
}

function renderRecordingControls() {
  const locked = pendingCommand || isBusy();
  record.disabled = pendingCommand
    || (phase !== "recording" && (locked || loadingAudioInputs || !audioInput.value));
  language.disabled = locked;
  audioInput.disabled = refreshInputs.disabled = locked || loadingAudioInputs;
}

function showInputName() {
  const name = audioInput.selectedOptions[0]?.text ?? "";
  audioInput.title = name;
}

function appendProcessingLog(message, tone = "") {
  const item = document.createElement("li");
  if (tone) item.dataset.tone = tone;
  const timestamp = document.createElement("time");
  const now = new Date();
  timestamp.dateTime = now.toISOString();
  timestamp.textContent = logTimeFormat.format(now);
  const text = document.createElement("span");
  text.textContent = message;
  item.append(timestamp, text);
  processingLog.append(item);
  while (processingLog.children.length > 10) processingLog.firstElementChild.remove();
  processingLog.scrollTop = processingLog.scrollHeight;
}

function clearProcessingLog() {
  processingLog.replaceChildren();
}

function renderProcessingStatus(state, changed, previousPhase, beginsAttempt = false) {
  const nextPhase = state.phase;
  const beginsJob = beginsAttempt || changed
    && ["idle", "done", "failed"].includes(previousPhase)
    && ["starting", "preparing", "extracting"].includes(nextPhase);
  if (beginsJob) {
    clearProcessingLog();
    processingProgressValue = 0;
    jobStartedAt = performance.now();
  }
  if (nextPhase !== "idle") byId("log-navigation").hidden = false;
  if (nextPhase !== "idle" && !jobStartedAt) jobStartedAt = performance.now();
  byId("processing-session").textContent = state.session || (state.captions ? "Caption job" : "New recording");
  if (jobStartedAt) byId("job-elapsed").textContent = `Elapsed ${clock((performance.now() - jobStartedAt) / 1000)}`;
  logResume.hidden = !state.session || state.captions || nextPhase !== "failed";
  logResume.disabled = pendingCommand || isBusy();
  logRestart.hidden = !state.session || state.captions;
  logRestart.disabled = !canRestart(state.session);
  byId("engine-details").hidden = !state.engine_log;
  byId("engine-log").textContent = state.engine_log || "";
  if (phaseProgress[nextPhase] !== undefined) {
    processingProgressValue = phaseProgress[nextPhase];
  }

  processingStatus.dataset.phase = nextPhase;
  processingState.textContent = phases[nextPhase];
  processingDetail.textContent = state.restarting ? "Stopping the engine before restarting transcription…" : state.error || phaseDetails[nextPhase];
  processingPercent.textContent = `${processingProgressValue}%`;
  processingProgress.setAttribute("aria-valuenow", processingProgressValue);
  processingProgress.setAttribute("aria-valuetext", phases[nextPhase]);
  processingProgressBar.style.width = `${processingProgressValue}%`;

  if (changed && nextPhase !== "idle") {
    appendProcessingLog(
      state.error || phaseDetails[nextPhase],
      nextPhase === "failed" ? "error" : nextPhase === "done" ? "success" : "",
    );
  }
}

function renderAudioInputs(inputs, selectDefault) {
  const options = [new Option("System default", "default")];
  for (const { id, name } of inputs) options.push(new Option(name, id));
  if (!selectDefault) {
    const prompt = new Option("Select an audio input", "");
    prompt.disabled = true;
    options.unshift(prompt);
  }
  audioInput.replaceChildren(...options);
  audioInput.value = selectDefault ? "default" : "";
  showInputName();
}

async function refreshAudioInputs(captureError = "") {
  if (loadingAudioInputs) return;
  loadingAudioInputs = true;
  const selectDefault = !captureError && audioInput.value === "default";
  if (captureError) renderAudioInputs([], false);
  renderRecordingControls();
  try {
    renderAudioInputs(await invoke("list_audio_inputs"), selectDefault);
  } catch (error) {
    const detail = `Could not refresh audio inputs. ${error}`;
    showMessage(captureError ? `${captureError}\n${detail}` : detail);
  } finally {
    loadingAudioInputs = false;
    renderRecordingControls();
  }
}

function applyPreferences(next) {
  document.documentElement.dataset.appearance = next.appearance;
  if (next.backdrop === "solid") delete document.documentElement.dataset.backdrop;
  else document.documentElement.dataset.backdrop = next.backdrop;
  if (preferences?.spoken_language !== next.spoken_language) {
    language.value = next.spoken_language;
    captionLanguage.value = next.spoken_language;
  }
  preferences = next;
}

function setSettingsBusy(busy) {
  for (const fieldset of settingsFields) fieldset.disabled = busy;
  settingsSave.disabled = busy;
}

async function loadSettings() {
  setSettingsBusy(true);
  try {
    const result = await invoke("get_settings");
    for (const option of backdropSetting.options) {
      option.hidden = !result.backdrops.includes(option.value);
      option.disabled = option.hidden;
    }
    backdropSetting.disabled = result.backdrops.length === 1;
    if (backdropSetting.disabled) {
      renderText("backdrop-hint", "Window effects are unavailable on this system.");
    }
    appearanceSetting.value = result.preferences.appearance;
    backdropSetting.value = result.preferences.backdrop;
    languageSetting.value = result.preferences.spoken_language;
    applyPreferences(result.preferences);
    setSettingsBusy(false);
  } catch (error) {
    settingsStatus.textContent = `Could not load settings. ${error}`;
  }
}

async function saveSettings(event) {
  event.preventDefault();
  if (settingsSave.disabled) return;
  const next = {
    appearance: appearanceSetting.value,
    backdrop: backdropSetting.value,
    spoken_language: languageSetting.value,
  };
  setSettingsBusy(true);
  settingsStatus.textContent = "Saving…";
  try {
    await invoke("save_settings", { preferences: next });
    applyPreferences(next);
    settingsStatus.textContent = "Settings saved.";
  } catch (error) {
    settingsStatus.textContent = String(error);
  } finally {
    setSettingsBusy(false);
  }
}

async function pickCaptionMedia() {
  if (captionBrowse.disabled) return;
  pickingCaptionMedia = true;
  captionBrowse.disabled = true;
  try {
    const path = await invoke("pick_caption_media");
    if (path !== null) {
      captionMedia.value = path;
      if (workspace.dataset.view === "captions") captionMedia.focus({ preventScroll: true });
    }
  } catch (error) {
    showMessage(String(error));
  } finally {
    pickingCaptionMedia = false;
    captionBrowse.disabled = captionMedia.disabled;
  }
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

function renderSessionList(container, entries) {
  const isRecent = container === recent;
  const focused = container.contains(document.activeElement) ? document.activeElement : null;
  const items = document.createDocumentFragment();
  let focusedButton;
  for (const session of entries) {
    const date = new Date(session.timestamp);
    const row = document.createElement("div");
    row.className = "session-row";
    row.dataset.id = session.id;
    const button = document.createElement("button");
    button.type = "button";
    button.className = "session";
    button.dataset.id = session.id;
    if (!isRecent && session.id === selected?.id) button.setAttribute("aria-current", "true");
    if (isRecent) {
      const icon = document.createElement("span");
      icon.className = "icon icon-library";
      icon.setAttribute("aria-hidden", "true");
      button.append(icon);
    }
    const info = document.createElement("span");
    info.className = "session-info";
    const name = document.createElement("strong");
    name.textContent = (isRecent ? dateFormat : sessionDateFormat).format(date);
    if (session.favorite) {
      const star = document.createElement("span");
      star.className = "session-favorite";
      star.setAttribute("role", "img");
      star.setAttribute("aria-label", "Favorite");
      star.textContent = " ★";
      name.append(star);
    }
    const detail = document.createElement("small");
    detail.textContent = isRecent ? timeFormat.format(date) : `${timeFormat.format(date)} · ${sessionStatus(session)}`;
    info.append(name, detail);
    button.append(info);
    if (isRecent) {
      const status = document.createElement("span");
      status.className = "session-status";
      status.textContent = sessionStatus(session);
      button.append(status);
    }
    const actions = document.createElement("button");
    actions.type = "button";
    actions.className = "session-actions";
    actions.dataset.id = session.id;
    actions.setAttribute("aria-label", `Actions for ${titleFormat.format(date)}`);
    actions.setAttribute("aria-haspopup", "menu");
    actions.setAttribute("aria-controls", "session-menu");
    actions.setAttribute("aria-expanded", "false");
    actions.textContent = "…";
    if (session.id === focused?.dataset.id) {
      focusedButton = focused.classList.contains("session-actions") ? actions : button;
    }
    row.append(button, actions);
    items.append(row);
  }
  if (entries.length === 0) {
    const label = document.createElement("p");
    label.className = "list-empty";
    label.textContent = "No recordings yet";
    items.append(label);
  }
  container.replaceChildren(items);
  focusedButton?.focus({ preventScroll: true });
}

async function refreshHistory(preferred) {
  sessions = await invoke("history");
  const next = sessions.find(({ id }) => id === (preferred ?? selected?.id)) ?? sessions[0];
  closeSessionMenu(true);
  const ordered = [...sessions].sort((left, right) => Number(right.favorite) - Number(left.favorite));
  renderSessionList(history, ordered);
  renderSessionList(recent, sessions.slice(0, 3));
  renderText("session-count", sessions.length ? String(sessions.length) : "");
  if (next && (!opened || preferred || next.id !== selected?.id)) await openSession(next);
  else if (next) renderText("session-state", sessionStatus(next));
  else if (selected) {
    ++request;
    selected = opened = null;
    unloadAudio();
    review.hidden = true;
    empty?.removeAttribute("hidden");
  }
}

function unloadAudio() {
  pendingSeek = null;
  audio.removeAttribute("src");
  audio.load();
  byId("player").hidden = true;
  renderPlayer();
}

async function openSession(session) {
  const reading = ++request;
  selected = session;
  opened = null;
  for (const button of history.querySelectorAll(".session")) {
    if (button.dataset.id === session.id) button.setAttribute("aria-current", "true");
    else button.removeAttribute("aria-current");
  }
  unloadAudio();
  renderText("session-state", sessionStatus(session));
  renderText("session-title", titleFormat.format(new Date(session.timestamp)));
  empty?.setAttribute("hidden", "");
  review.hidden = false;
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
    article.textContent = `Could not open the session. ${error}`;
  }
}

function closeSessionMenu(restoreFocus = false) {
  if (sessionMenu.hidden) return;
  sessionMenu.hidden = true;
  menuOpener?.setAttribute("aria-expanded", "false");
  if (restoreFocus) menuOpener?.focus({ preventScroll: true });
  menuOpener = null;
}

function openSessionMenu(row, point) {
  if (!row || mutatingSession) return;
  const session = sessions.find(({ id }) => id === row.dataset.id);
  if (!session) return;
  closeSessionMenu();
  menuOpener = row.querySelector(".session-actions");
  menuOpener.setAttribute("aria-expanded", "true");
  for (const item of menuItems) {
    switch (item.dataset.action) {
      case "favorite": item.textContent = session.favorite ? "Remove from favorites" : "Add to favorites"; break;
      case "summary": item.disabled = !session.has_summary; break;
      case "transcript": item.disabled = !session.has_transcript; break;
      case "delete": item.disabled = isBusy() || pendingCommand; break;
    }
  }
  sessionMenu.hidden = false;
  const anchor = menuOpener.getBoundingClientRect();
  const { offsetWidth: width, offsetHeight: height } = sessionMenu;
  const x = point?.x ?? anchor.right - width;
  const y = point?.y ?? (anchor.bottom + height > window.innerHeight ? anchor.top - height : anchor.bottom);
  sessionMenu.style.left = `${Math.max(6, Math.min(x, window.innerWidth - width - 6))}px`;
  sessionMenu.style.top = `${Math.max(6, Math.min(y, window.innerHeight - height - 6))}px`;
  menuItems[0].focus({ preventScroll: true });
}

async function performSessionAction(event) {
  const action = event.target.closest("[data-action]")?.dataset.action;
  const session = sessions.find(({ id }) => id === menuOpener?.dataset.id);
  if (!action || !session) return;
  closeSessionMenu(true);
  showMessage();
  if (action === "delete") {
    pendingDeletion = session;
    const date = titleFormat.format(new Date(session.timestamp));
    byId("delete-description").textContent = `The recording from ${date} and all its files will be deleted permanently.`;
    deleteDialog.returnValue = "";
    deleteDialog.showModal();
    return;
  }
  try {
    if (action === "favorite") {
      mutatingSession = true;
      await invoke("set_favorite", { id: session.id, favorite: !session.favorite });
      await refreshHistory();
    } else {
      await invoke("open_session_source", { id: session.id, source: action });
    }
  } catch (error) {
    showMessage(String(error));
  } finally {
    if (action === "favorite") mutatingSession = false;
  }
}

async function confirmSessionDeletion() {
  const session = pendingDeletion;
  pendingDeletion = null;
  if (!session || deleteDialog.returnValue !== "delete") return;
  if (isBusy() || pendingCommand) {
    showMessage("Wait for the active job to finish before deleting a session.");
    return;
  }
  mutatingSession = true;
  if (session.id === selected?.id) {
    ++request;
    opened = null;
    unloadAudio();
  }
  try {
    await invoke("delete_session", { id: session.id });
    await refreshHistory();
    if (document.activeElement === document.body) viewHeading?.focus({ preventScroll: true });
    showMessage("Session deleted.");
  } catch (error) {
    await refreshHistory().catch(() => {});
    showMessage(String(error));
  } finally {
    mutatingSession = false;
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
  restart.hidden = !opened;
  restart.disabled = !canRestart(selected?.id);
  const data = opened?.[kind];
  if (data) {
    // The Rust backend renders Markdown and restricts HTML and links.
    article.innerHTML = data.html;
    if (kind === "transcript") {
      article.querySelectorAll("p").forEach((paragraph, index) => {
        paragraph.id = `transcript-paragraph-${index}`;
      });
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
  renderText("current-time", clock(audio.currentTime));
  renderText("duration", clock(audio.duration));
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
  const paragraph = opened.transcript.segment_paragraphs?.[Number(target[1]) - 1];
  const heading = document.getElementById(paragraph === undefined
    ? `segment-${target[1]}`
    : `transcript-paragraph-${paragraph}`);
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
  const amplitude = recording ? sample : 0;
  if (level) level.value = amplitude;
  if (!signalContext) return;
  const { width, height } = signalChart;
  signalContext.clearRect(0, 0, width, height);
  if (recording) {
    signalSamples.copyWithin(0, 1);
    signalSamples[signalSamples.length - 1] = amplitude;
  } else {
    signalSamples.fill(0);
  }
  const step = width / signalSamples.length;
  const style = getComputedStyle(signalChart);
  signalContext.fillStyle = style.color;
  for (let x = step / 2; x < width; x += step * 4) {
    signalContext.fillRect(x, 0, 1, height);
  }
  signalContext.fillRect(0, height / 2, width, 1);
  signalContext.fillStyle = style.getPropertyValue("--wave").trim();
  signalContext.globalAlpha = recording ? 1 : .52;
  for (const [index, value] of signalSamples.entries()) {
    const bar = Math.max(4, Math.sqrt(value) * height * .9);
    signalContext.fillRect((index + .38) * step, (height - bar) / 2, step * .24, bar);
  }
  signalContext.globalAlpha = 1;
}

function renderStatus(state) {
  const attemptChanged = state.attempt !== undefined && state.attempt !== jobState.attempt;
  if (attemptChanged && isBusy() && jobStartedAt) {
    appendProcessingLog("Starting a new transcription attempt. Previous results are backed up.");
    jobStartedAt = performance.now();
  }
  const restartChanged = state.restarting && !jobState.restarting;
  jobState = state;
  if (restartChanged) appendProcessingLog("Restart requested. Waiting for the engine to stop.");
  const recording = state.phase === "recording";
  const previousPhase = phase;
  const changed = phase !== state.phase || attemptChanged;
  if (recording && changed) recordingSince = performance.now();
  phase = state.phase;
  const beginsAttempt = attemptChanged && state.phase !== "idle" && ["idle", "done", "failed"].includes(previousPhase);
  renderProcessingStatus(state, changed, previousPhase, beginsAttempt);
  updateActiveJob(recording || phase === "starting" ? "record" : "log");
  const recordLabel = recording ? "Stop recording" : "Start recording";
  renderText("record-label", recordLabel);
  record.setAttribute("aria-label", recordLabel);
  record.dataset.recording = String(recording);
  renderRecordingControls();
  retry.disabled = pendingCommand || isBusy();
  restart.disabled = !canRestart(selected?.id);
  const captionBusy = state.captions && isBusy();
  captionMedia.disabled = captionBusy;
  captionBrowse.disabled = captionBusy || pickingCaptionMedia;
  captionLanguage.disabled = captionBusy;
  captionTrack.disabled = captionBusy;
  captionSubmit.disabled = pendingCommand || isBusy();
  if (state.captions) {
    captionStatus.textContent = state.error || (state.subtitle_path ? "Subtitles saved." : phases[phase]);
    captionOutput.hidden = !state.subtitle_path;
    captionOutput.value = state.subtitle_path || "";
  }
  if (phase === "starting") renderText("elapsed", "00:00");
  if (recording) {
    renderText("elapsed", clock((performance.now() - recordingSince) / 1000));
  }
  renderSignal(recording, state.level);
  if (!state.captions && !selected && changed) {
    renderText("empty-title", isBusy() ? phases[phase] : "No sessions yet");
    renderText("empty-description", recording
      ? "Select Stop to save the audio and start processing."
      : isBusy() ? "The session will appear in History when processing finishes."
        : "Your recordings, transcripts and summaries will appear here.");
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
      if (state.error && !state.captions) {
        showMessage(state.error);
        if (state.refresh_audio_inputs && (force || previous !== phase)) {
          void refreshAudioInputs(state.error);
        }
      }
      if (!state.captions || !selected) await refreshHistory(state.session);
    }
  } catch (error) {
    const detail = String(error);
    showMessage(detail);
    appendProcessingLog(detail, "error");
  }
}

async function command(name, args) {
  if (pendingCommand || mutatingSession) return;
  pendingCommand = true;
  pollGeneration++;
  renderRecordingControls();
  retry.disabled = true;
  restart.disabled = logRestart.disabled = logResume.disabled = true;
  captionSubmit.disabled = true;
  clearTimeout(pollTimeout);
  showMessage();
  let submitted = false;
  try {
    await invoke(name, args);
    submitted = true;
  } catch (error) {
    const detail = String(error);
    showMessage(detail);
    appendProcessingLog(detail, "error");
  } finally {
    pendingCommand = false;
    await poll(submitted);
  }
}

for (const button of document.querySelectorAll("[data-view-target]")) {
  button.addEventListener("click", () => showView(button.dataset.viewTarget));
}
function selectSession(event) {
  const row = event.target.closest(".session-row");
  if (!row || mutatingSession) return;
  if (event.target.closest(".session-actions")) {
    if (menuOpener === row.querySelector(".session-actions")) closeSessionMenu(true);
    else openSessionMenu(row);
    return;
  }
  const button = event.target.closest(".session");
  if (!button) return;
  const session = sessions.find(({ id }) => id === button.dataset.id);
  if (session) {
    if (session.id !== selected?.id || !opened) void openSession(session);
    showView("sessions");
  }
}
for (const list of [history, recent]) {
  list.addEventListener("click", selectSession);
  list.addEventListener("contextmenu", (event) => {
    const row = event.target.closest(".session-row");
    if (!row) return;
    event.preventDefault();
    openSessionMenu(row, event.button === 2 ? { x: event.clientX, y: event.clientY } : undefined);
  });
  list.addEventListener("keydown", (event) => {
    if (event.key !== "ContextMenu" && !(event.shiftKey && event.key === "F10")) return;
    const row = event.target.closest(".session-row");
    if (!row) return;
    event.preventDefault();
    openSessionMenu(row);
  });
}
sessionMenu.addEventListener("click", performSessionAction);
sessionMenu.addEventListener("keydown", (event) => {
  if (event.key === "Escape" || event.key === "Tab") {
    if (event.key === "Escape") event.preventDefault();
    closeSessionMenu(true);
    return;
  }
  const available = menuItems.filter((item) => !item.disabled);
  const current = available.indexOf(document.activeElement);
  let next;
  switch (event.key) {
    case "ArrowDown": next = (current + 1) % available.length; break;
    case "ArrowUp": next = (current + available.length - 1) % available.length; break;
    case "Home": next = 0; break;
    case "End": next = available.length - 1; break;
    default: return;
  }
  event.preventDefault();
  available[next].focus();
});
document.addEventListener("pointerdown", (event) => {
  if (!sessionMenu.contains(event.target) && !menuOpener?.contains(event.target)) closeSessionMenu();
});
document.addEventListener("scroll", (event) => {
  if (event.target !== sessionMenu) closeSessionMenu();
}, true);
window.addEventListener("resize", () => closeSessionMenu(true));
deleteDialog.addEventListener("close", confirmSessionDeletion);
audioInput.addEventListener("change", () => {
  showInputName();
  renderRecordingControls();
});
refreshInputs.addEventListener("click", () => void refreshAudioInputs());
record.addEventListener("click", () => {
  if (phase === "recording") void command("stop_recording");
  else if (!record.disabled) {
    void command("start_recording", {
      language: language.value,
      inputId: audioInput.value === "default" ? null : audioInput.value,
    });
  }
});
captionBrowse.addEventListener("click", pickCaptionMedia);
setupModelControls();
byId("cancel-model-download").addEventListener("click", () => void modelAction("cancel_model_download", {}));
deleteModelDialog.addEventListener("close", () => {
  const id = pendingModelDeletion;
  pendingModelDeletion = null;
  if (deleteModelDialog.returnValue === "delete" && id) void modelAction("delete_model", { id });
});
settingsForm.addEventListener("submit", saveSettings);
settingsForm.addEventListener("change", () => { settingsStatus.textContent = ""; });
byId("captions-form").addEventListener("submit", (event) => {
  event.preventDefault();
  const media = captionMedia.value.trim().replace(/^"(.*)"$/, "$1");
  if (!media || isBusy() || pendingCommand) return;
  void command("generate_captions", {
    media, language: captionLanguage.value, audioTrack: Number(captionTrack.value),
  });
});
retry.addEventListener("click", () => {
  if (selected && !isBusy()) {
    showView("log");
    void command("retry", { id: selected.id, language: language.value });
  }
});
function canRestart(id) {
  return Boolean(id) && !pendingCommand && !jobState.restarting && (!isBusy()
    || (!jobState.captions && jobState.session === id && ["preparing", "transcribing", "synthesizing"].includes(phase)));
}
function requestRestart(id) {
  if (!canRestart(id)) return;
  pendingRestart = { id, language: language.value };
  restartDialog.returnValue = "";
  restartDialog.showModal();
}
restart.addEventListener("click", () => requestRestart(selected?.id));
logRestart.addEventListener("click", () => requestRestart(jobState.session));
logResume.addEventListener("click", () => {
  if (jobState.session && !isBusy()) void command("retry", { id: jobState.session, language: language.value });
});
restartDialog.addEventListener("close", () => {
  const args = pendingRestart;
  pendingRestart = null;
  if (restartDialog.returnValue === "restart" && args && canRestart(args.id)) {
    showView("log");
    void command("restart", args);
  }
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
void window.__TAURI__.event.listen("model-download-active", () => {
  showView("settings");
  showMessage("Finish or cancel the download before closing Lovelace.");
}).catch((error) => console.error(error));
void window.__TAURI__.webview.getCurrentWebview().onDragDropEvent(({ payload }) => {
  if (payload.type !== "drop" || workspace.dataset.view !== "captions" || captionMedia.disabled) return;
  if (payload.paths.length !== 1) {
    showMessage("Drop one media file at a time.");
    return;
  }
  showMessage();
  captionMedia.value = payload.paths[0];
  captionMedia.focus();
}).catch((error) => console.error(error));
void poll();
void loadSettings();
void refreshModels();
void refreshAudioInputs();

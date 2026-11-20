// Model management actions without a WebView, network or installed models.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
class Element {
  constructor() {
    this.dataset = {}; this.children = []; this.options = []; this.value = '';
    this.style = {}; this.attributes = {}; this.listeners = {}; this.disabled = false;
    this.classList = { contains: () => false, add: () => {} };
  }
  querySelectorAll() { return []; }
  querySelector() { return null; }
  addEventListener(name, fn) { this.listeners[name] = fn; }
  setAttribute(key, value) { this.attributes[key] = value; }
  removeAttribute(key) { delete this.attributes[key]; }
  append(...nodes) { this.children.push(...nodes); }
  replaceChildren(...nodes) { this.children = nodes; this.options = nodes; }
  focus() {}
  load() {}
  getContext() { return null; }
  showModal() { this.open = true; }
}
const html = fs.readFileSync('ui/index.html', 'utf8');
const nodes = new Map([...html.matchAll(/id="([^"]+)"/g)].map(match => [match[1], new Element()]));
const workspace = new Element(); workspace.dataset.view = 'settings';
const document = {
  querySelector: () => workspace, querySelectorAll: () => [],
  getElementById: id => nodes.get(id), createElement: () => new Element(), addEventListener: () => {},
  documentElement: new Element(), body: new Element(),
};
const calls = [];
let pickerPath = null;
const kinds = ['whisper_model', 'llama_model', 'whisper_engine', 'llama_engine'];
const snapshot = {
  catalog: kinds.map((kind, i) => ({ id: ['whisper-small', 'qwen-3b', 'whisper-engine', 'llama-engine'][i],
    kind, label: kind, size: 1000000, license: 'MIT', source: 'https://example.test', path: null })),
  active: kinds.map(kind => ({ kind, path: `/external/${kind}`, exists: true })),
  directory: '/managed', job_busy: false, download: { id: '', stage: '' },
};
const invoke = async (name, args) => {
  calls.push({ name, args });
  if (name === 'get_models') return structuredClone(snapshot);
  if (name === 'pick_model_file') return pickerPath;
};
const context = vm.createContext({ document, window: {
  __TAURI__: { core: { invoke, convertFileSrc: x => x }, event: { listen: async () => {} },
    webview: { getCurrentWebview: () => ({ onDragDropEvent: async () => {} }) } }, addEventListener: () => {},
}, Intl, Date, Map, Float32Array, performance, console, setTimeout: () => 0, clearTimeout: () => {}, navigator: {},
Option: class extends Element { constructor(text, value) { super(); this.textContent = text; this.value = value; } },
});
vm.runInContext(fs.readFileSync('ui/app.js', 'utf8').replace(/void poll\(\);[\s\S]*$/, ''), context);
const run = code => vm.runInContext(code, context);
const plain = value => JSON.parse(JSON.stringify(value));
(async () => {
  await run('refreshModels()');
  const controls = run("modelGroups.get('whisper_model')");
  assert.equal(controls.select.value, 'whisper-small');
  assert.equal(controls.install.textContent, 'Download & use');
  assert.equal(controls.remove.disabled, true);
  assert.match(controls.path.textContent, /external/);
  const engines = run("modelGroups.get('llama_engine')");
  snapshot.catalog.push({ id: 'llama-cuda', kind: 'llama_engine', label: 'CUDA 12.4',
    size: 1000000, download_size: 501000000, notes: 'NVIDIA GPU and compatible driver. CUDA runtime included.',
    license: 'MIT', source: 'https://example.test', path: null });
  await run('refreshModels()');
  engines.select.value = 'llama-cuda';
  engines.select.listeners.change();
  assert.match(engines.requirements.textContent, /NVIDIA/);
  assert.match(engines.select.options.find(option => option.value === 'llama-cuda').textContent, /501.0 MB/);
  engines.install.listeners.click();
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(plain(calls.find(call => call.name === 'download_model')), { name: 'download_model', args: { id: 'llama-cuda' } });
  assert.equal(engines.select.value, 'llama-cuda');
  calls.length = 0;
  await run("modelAction('download_model', {id:'whisper-small'})");
  assert.deepEqual(plain(calls.find(call => call.name === 'download_model')), { name: 'download_model', args: { id: 'whisper-small' } });

  snapshot.catalog[0].path = '/managed/small.bin';
  snapshot.active[0].path = '/managed/small.bin';
  await run('refreshModels()');
  assert.equal(controls.install.textContent, 'Selected');
  assert.equal(controls.install.disabled, true);
  assert.equal(controls.remove.disabled, false);
  controls.remove.listeners.click();
  assert.equal(nodes.get('delete-model').open, true);
  nodes.get('delete-model').returnValue = 'cancel';
  nodes.get('delete-model').listeners.close();
  assert.equal(calls.some(call => call.name === 'delete_model'), false);
  controls.remove.listeners.click();
  nodes.get('delete-model').returnValue = 'delete';
  nodes.get('delete-model').listeners.close();
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(plain(calls.find(call => call.name === 'delete_model')), { name: 'delete_model', args: { id: 'whisper-small' } });

  pickerPath = '/another/custom.gguf';
  await run("browseModel('llama_model')");
  assert.deepEqual(plain(calls.find(call => call.name === 'use_model_path')), { name: 'use_model_path', args: { kind: 'llama_model', path: pickerPath } });
  const count = calls.filter(call => call.name === 'use_model_path').length;
  pickerPath = null;
  await run("browseModel('llama_model')");
  assert.equal(calls.filter(call => call.name === 'use_model_path').length, count);

  snapshot.download = { id: 'whisper-small', stage: 'downloading', received: 500000, total: 1000000 };
  await run('refreshModels()');
  assert.equal(nodes.get('model-transfer-progress').value, 50);
  assert.equal(nodes.get('cancel-model-download').hidden, false);
  assert.equal(controls.browse.disabled, true);
  assert.equal(controls.remove.disabled, true);
  await run("modelAction('cancel_model_download', {})");
  assert.equal(calls.some(call => call.name === 'cancel_model_download'), true);
  snapshot.download.stage = 'failed'; snapshot.download.error = 'Checksum mismatch';
  await run('refreshModels()');
  assert.equal(nodes.get('model-transfer-status').textContent, 'Checksum mismatch');
  assert.equal(controls.browse.disabled, false);
  snapshot.job_busy = true;
  await run('refreshModels()');
  assert.equal(controls.install.disabled, true);
  assert.equal(controls.browse.disabled, true);
  assert.equal(controls.remove.disabled, true);
  console.log('Model UI passed: catalog, activation, browse/cancel, deletion confirmation, progress, retry and busy guards.');
})().catch(error => { console.error(error); process.exitCode = 1; });

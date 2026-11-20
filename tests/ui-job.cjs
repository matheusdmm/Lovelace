// Exercise the UI state and actions without a WebView, microphone or models.
const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
class Element {
  constructor() {
    this.dataset = {}; this.hidden = false; this.children = []; this.options = [];
    this.style = {}; this.attributes = {}; this.value = ''; this.classList = {contains:()=>false,add:()=>{}};
    this.listeners = {};
  }
  querySelectorAll() { return []; }
  querySelector() { return null; }
  addEventListener(name, fn) { this.listeners[name] = fn; }
  setAttribute(key,value) { this.attributes[key] = value; }
  removeAttribute(key) { delete this.attributes[key]; }
  getAttribute(key) { return this.attributes[key]; }
  hasAttribute(key) { return key in this.attributes; }
  append(...nodes) { this.children.push(...nodes); }
  replaceChildren(...nodes) { this.children = nodes; }
  focus() {}
  load() {}
  getContext() { return null; }
  showModal() { this.open = true; }
}
const html = fs.readFileSync('ui/index.html','utf8');
const nodes = new Map([...html.matchAll(/id="([^"]+)"/g)].map(m=>[m[1],new Element()]));
const panels = [...html.matchAll(/data-panel="([^"]+)"/g)].map(m=>Object.assign(new Element(),{dataset:{panel:m[1]},hidden:m[1]!=='record'}));
const navigation = [...html.matchAll(/class="nav-item" data-view-target="([^"]+)"( hidden)?/g)].map(m=>Object.assign(new Element(),{dataset:{viewTarget:m[1]},hidden:!!m[2]}));
const workspace = new Element(); workspace.dataset.view = 'record'; workspace.querySelectorAll=()=>panels;
nodes.get('log-navigation').hidden = true;
const document = {
  querySelector:()=>workspace,
  querySelectorAll:(selector)=>selector==='.nav-item'||selector==='[data-view-target]'?navigation:[],
  getElementById:id=>nodes.get(id), createElement:()=>new Element(), addEventListener:()=>{},
  documentElement:new Element(), body:new Element(),
};
const calls = [];
const invoke = async(name,args)=>{calls.push({name,args}); return name==='status'?{phase:'transcribing',session:'sessao-1-0',attempt:2}:[];};
const context = vm.createContext({document,window:{__TAURI__:{core:{invoke,convertFileSrc:x=>x},event:{listen:async()=>{}},webview:{getCurrentWebview:()=>({onDragDropEvent:async()=>{}})}}},
  Intl,Date,Map,Float32Array,performance,console,setTimeout:()=>0,clearTimeout:()=>{},navigator:{},Option:Element});
const app = fs.readFileSync('ui/app.js','utf8').replace(/void poll\(\);[\s\S]*$/, '');
context.window.addEventListener = () => {};
vm.runInContext(app,context);
assert.equal(nodes.get('log-navigation').hidden,true);
vm.runInContext("renderStatus({phase:'idle',attempt:0})",context);
assert.equal(nodes.get('processing-log').children.length,0);
vm.runInContext("renderStatus({phase:'transcribing',session:'sessao-1-0',attempt:1,engine_log:'progress = 15%'})",context);
assert.equal(nodes.get('log-navigation').hidden,false);
assert.equal(nodes.get('engine-log').textContent,'progress = 15%');
assert.equal(nodes.get('log-restart').disabled,false);
assert.equal(nodes.get('processing-log').children.length,1);
assert.equal(nodes.get('log-resume').hidden,true);
assert.equal(vm.runInContext("canRestart('sessao-2-0')",context),false);
vm.runInContext("showView('log'); requestRestart('sessao-1-0')",context);
assert.equal(nodes.get('restart-session').open,true);
assert.equal(workspace.dataset.view,'log');
assert.equal(panels.find(p=>p.dataset.panel==='record').hidden,true);
nodes.get('restart-session').returnValue='cancel';
nodes.get('restart-session').listeners.close();
assert.equal(calls.length,0);
vm.runInContext("renderStatus({phase:'synthesizing',session:'sessao-1-0',attempt:1,restarting:true})",context);
assert.equal(nodes.get('log-restart').disabled,true);
assert.match(nodes.get('processing-detail').textContent,/Stopping/);
vm.runInContext("renderStatus({phase:'failed',session:'sessao-1-0',attempt:2,error:'Engine failed'})",context);
assert.equal(nodes.get('log-resume').hidden,false);
assert.equal(nodes.get('log-resume').disabled,false);
assert.equal(nodes.get('log-navigation').hidden,false);
vm.runInContext("renderStatus({phase:'synthesizing',session:'sessao-1-0',attempt:3})",context);
assert.equal(nodes.get('processing-log').children.length,1);
assert.equal(nodes.get('processing-log').children[0].children[1].textContent,'Creating the digested summary from the transcript.');
vm.runInContext("renderStatus({phase:'done',session:'sessao-1-0',attempt:2})",context);
assert.equal(nodes.get('log-resume').hidden,true);
assert.equal(nodes.get('log-restart').disabled,false);
assert.match(html, /data-panel="log"[\s\S]*id="processing-status"/);
assert.equal(html.slice(html.indexOf('data-panel="record"'),html.indexOf('data-panel="sessions"')).includes('id="processing-status"'),false);
vm.runInContext("requestRestart('sessao-1-0')",context);
nodes.get('restart-session').returnValue='restart';
nodes.get('restart-session').listeners.close();
assert.deepEqual(JSON.parse(JSON.stringify(calls[0])), {name:'restart',args:{id:'sessao-1-0',language:''}});
console.log('UI job states passed: hidden navigation, dedicated log, progress, restart guards, confirmation cancellation, failure/resume, completion.');

// A timestamp can refer to any segment inside a continuous paragraph.
const paragraphs = [new Element(), new Element()];
let scrolledTo = null;
paragraphs.forEach((paragraph, index) => {
  paragraph.scrollIntoView = () => { scrolledTo = index; };
});
nodes.get('document').querySelectorAll = selector => selector === 'p' ? paragraphs : [];
vm.runInContext(`opened = {
  summary: {markdown: '# Summary', html: '<h1>Summary</h1>'},
  transcript: {
    markdown: '# Transcript\\n\\nA complete thought.\\n\\nAnother paragraph.\\n',
    html: '<h1>Transcript</h1><p>A complete thought.</p><p>Another paragraph.</p>',
    segment_paragraphs: [0, 0, 1]
  }
}; showDocument('transcript');`, context);
paragraphs.forEach(paragraph => nodes.set(paragraph.id, paragraph));
nodes.get('audio').duration = 120;
let prevented = false;
context.excerptEvent = {
  preventDefault: () => { prevented = true; },
  target: {closest: () => ({
    getAttribute: () => 'transcricao.md#segment-0002', textContent: '00:00:09.460'
  })}
};
vm.runInContext("showDocument('summary'); jumpToExcerpt(excerptEvent)", context);
assert.equal(prevented, true);
assert.equal(scrolledTo, 0);
assert.equal(nodes.get('audio').currentTime, 9.46);
context.excerptEvent.target.closest = () => ({
  getAttribute: () => 'transcricao.md#trecho-0003', textContent: '00:01:02.345'
});
vm.runInContext('jumpToExcerpt(excerptEvent)', context);
assert.equal(scrolledTo, 1);
assert.equal(nodes.get('audio').currentTime, 62.345);
let copiedMarkdown;
context.navigator.clipboard = {writeText: async text => { copiedMarkdown = text; }};
vm.runInContext('copyMarkdown()', context).then(() => {
  assert.equal(copiedMarkdown, '# Transcript\n\nA complete thought.\n\nAnother paragraph.\n');
  console.log('Transcript actions passed: paragraph navigation, exact audio seeking, clean Markdown copying.');
}).catch(error => { console.error(error); process.exitCode = 1; });

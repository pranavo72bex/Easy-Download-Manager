// The extension is written in Rust (browser/src), compiled to WebAssembly.
// This loader is the one part that cannot be: a Manifest V3 background must
// register its listeners on its first turn, before the WebAssembly has
// loaded, so it registers them here and forwards each event once it has.
import init, * as dm from './pkg/dm_catcher.js';

const ext = globalThis.browser ?? globalThis.chrome;
const ready = init().then(() => dm);
// For a test harness attached to the worker; nothing in the extension uses it.
globalThis.dmReady = ready;

const forward = (name) => (...args) => {
  ready.then((m) => m[name](...args)).catch((e) => console.error(`dm: ${name}:`, e));
};

ext.downloads.onCreated.addListener(forward('on_download_created'));
ext.contextMenus.onClicked.addListener(forward('on_menu_clicked'));
ext.action.onClicked.addListener(forward('on_action_clicked'));
ext.runtime.onInstalled.addListener(forward('on_installed'));
ext.runtime.onStartup.addListener(forward('on_startup'));
// Media a page's player loads, for the panel's list (read, never changed).
ext.webRequest.onResponseStarted.addListener(
  forward('on_response_started'),
  { urls: ['<all_urls>'], types: ['main_frame', 'media', 'xmlhttprequest', 'other'] },
  ['responseHeaders'],
);
// The floating panel's click, answered so the panel can say what happened.
ext.runtime.onMessage.addListener((message, sender, reply) => {
  ready
    .then((m) => m.on_page_message(message, sender))
    .then(reply, (e) => reply({ ok: false, said: String(e) }));
  return true;
});

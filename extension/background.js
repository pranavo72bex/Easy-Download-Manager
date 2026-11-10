// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

// The extension is written in Rust (browser/src), compiled to WebAssembly.
// This loader is the one part that cannot be: a Manifest V3 background must
// register its listeners on its first turn, before the WebAssembly has
// loaded, so it registers them here and forwards each event once it has.
import init, * as dm from './pkg/dm_catcher.js';
import { WASM_SHA256 } from './pkg/build.js';

const ext = globalThis.browser ?? globalThis.chrome;
// Said to the page when this worker's code and the WebAssembly on disk are
// not from the same build, or the WebAssembly fails inside its glue.
const UPDATED = 'dm was updated: reload this page to use it';

// The WebAssembly is fetched here and checked against the digest built with
// the glue. The browser keeps this worker's scripts from when the extension
// was loaded but reads the .wasm from the folder each time the worker starts,
// so a rebuild in place (an unpacked copy) paired old glue with new
// WebAssembly and failed on its first call (gaps row 114). Then the extension
// is reloaded, which loads the matching glue: at most once in two minutes, so
// a folder caught halfway through a rebuild cannot set off a loop.
const load = async () => {
  const url = new URL('./pkg/dm_catcher_bg.wasm', import.meta.url);
  const bytes = await (await fetch(url)).arrayBuffer();
  const digest = [...new Uint8Array(await crypto.subtle.digest('SHA-256', bytes))]
    .map((b) => b.toString(16).padStart(2, '0')).join('');
  if (digest !== WASM_SHA256) {
    const { dmReloadedAt = 0 } = await ext.storage.local.get('dmReloadedAt');
    if (Date.now() - dmReloadedAt > 120000) {
      await ext.storage.local.set({ dmReloadedAt: Date.now() });
      ext.runtime.reload();
    }
    throw new Error(UPDATED);
  }
  await init({ module_or_path: bytes });
  return dm;
};
const ready = load();
// An error from inside the WebAssembly's glue is not one to show a person.
const plain = (e) => {
  const text = String(e);
  return text.includes(UPDATED) || /wasm|__wbg|wasm_bindgen/i.test(text) ? UPDATED : text;
};
// For a test harness attached to the worker; nothing in the extension uses it.
globalThis.dmReady = ready;

const forward = (name) => (...args) => {
  ready.then((m) => m[name](...args)).catch((e) => console.error(`dm: ${name}:`, e));
};

ext.downloads.onCreated.addListener(forward('on_download_created'));
// Chrome only: its choice of the file's name, held while dm decides, so a
// download dm takes never reaches the browser's Save dialog. `true` says the
// answer comes later; a failure to load still answers.
ext.downloads.onDeterminingFilename?.addListener((item, suggest) => {
  ready.then((m) => m.on_determining_filename(item, suggest))
    .catch((e) => { console.error('dm: on_determining_filename:', e); suggest(); });
  return true;
});
ext.contextMenus.onClicked.addListener(forward('on_menu_clicked'));
ext.action.onClicked.addListener(forward('on_action_clicked'));
ext.runtime.onInstalled.addListener(forward('on_installed'));
ext.runtime.onStartup.addListener(forward('on_startup'));
// A closed tab's streams are forgotten, in memory and in session storage.
ext.tabs.onRemoved.addListener(forward('on_tab_removed'));
// Media a page's player loads, for the panel's list (read, never changed).
ext.webRequest.onResponseStarted.addListener(
  forward('on_response_started'),
  { urls: ['<all_urls>'], types: ['main_frame', 'media', 'xmlhttprequest', 'other'] },
  ['responseHeaders'],
);
// The headers of the requests a page makes, read only (a Manifest V3 worker
// cannot change them), kept by address for the download a request begins.
// `extraHeaders` is Chrome's word for the ones it hides unless asked
// (Authorization, Origin); Firefox does not know it.
const sentTypes = { urls: ['<all_urls>'], types: ['main_frame', 'sub_frame', 'media', 'xmlhttprequest', 'other'] };
try {
  ext.webRequest.onSendHeaders.addListener(forward('on_send_headers'), sentTypes, ['requestHeaders', 'extraHeaders']);
} catch {
  ext.webRequest.onSendHeaders.addListener(forward('on_send_headers'), sentTypes, ['requestHeaders']);
}
// The body of a form a page sends: a download that is its answer is fetched
// by dm sending the same form. Which bodies are kept, and how, is decided in
// Rust (decide::posted_body); kept a moment by address, in the worker only.
ext.webRequest.onBeforeRequest.addListener(
  forward('on_before_request'),
  { urls: ['<all_urls>'], types: ['main_frame', 'sub_frame'] },
  ['requestBody'],
);

// Every message of the extension's pages, the page script's request for the
// panel's code among them, answered in Rust (web/page.rs). `true` keeps the
// answer's channel open until it is given.
ext.runtime.onMessage.addListener((message, sender, reply) => {
  ready
    .then((m) => m.on_page_message(message, sender))
    .then(reply, (e) => reply({ ok: false, said: plain(e) }));
  return true;
});

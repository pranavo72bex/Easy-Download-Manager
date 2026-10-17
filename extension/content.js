// The page script is written in Rust (browser/src/content.rs), compiled to
// WebAssembly. A content script cannot be a module, so this loads it: the
// one part that cannot be Rust.
(async () => {
  const ext = globalThis.browser ?? globalThis.chrome;
  const dm = await import(ext.runtime.getURL('pkg/dm_catcher.js'));
  await dm.default({ module_or_path: ext.runtime.getURL('pkg/dm_catcher_bg.wasm') });
  dm.content_start(ext);
})().catch((e) => console.debug('dm:', e));

// The toolbar popup is written in Rust (browser/src/popup.rs), compiled to
// WebAssembly; this loads it.
import init, * as dm from './pkg/dm_catcher.js';

const ext = globalThis.browser ?? globalThis.chrome;
init().then(() => dm.popup_start(ext)).catch((e) => console.error('dm:', e));

// The page script. Two small things are plain JavaScript, registered before
// anything loads, because they must work on a page whose security policy
// forbids WebAssembly: a click with a key held (Alt leaves a download with
// the browser, Shift hands it to dm, as the app's rules name them) and the
// page's links for "Download all/selected links".
// The "Download video" panel is written in Rust (browser/src/content.rs),
// compiled to WebAssembly; a content script cannot be a module, so this
// loads it. Message names are those in browser/src/decide.rs.
(() => {
  const ext = globalThis.browser ?? globalThis.chrome;
  // Put in again by the background when it found no page script answering
  // (a tab open before dm was installed or reloaded). One still connected
  // to dm stays; one cut off by a reload (its runtime has no id) is replaced.
  const before = globalThis.__dmPage;
  if (typeof before === 'function' ? before() : before) return;
  globalThis.__dmPage = () => {
    try { return Boolean(ext.runtime?.id); } catch { return false; }
  };

  // A click with a key held: the app's rules (Options › Browser) name the
  // key that leaves a download with the browser (Alt unless changed) and
  // the one that hands it to dm whatever its type or size (Shift unless
  // changed). The background is told before the download starts.
  // Capturing, so a page that stops the click on its way up cannot hide the
  // key. The take key is held to isTrusted, since it hands something over;
  // a made-up keep click can only leave a download with the browser.
  const keys = { keep: 'alt', take: 'shift' };
  const held = (event, key) => (key === 'alt' && event.altKey)
    || (key === 'shift' && event.shiftKey)
    || (key === 'ctrl' && (event.ctrlKey || event.metaKey));
  const readKeys = (rules) => {
    if (!rules || typeof rules !== 'object') return;
    if (typeof rules.keepKey === 'string') keys.keep = rules.keepKey;
    if (typeof rules.takeKey === 'string') keys.take = rules.takeKey;
  };
  try {
    ext.storage.local.get('dm.catch').then((got) => readKeys(got?.['dm.catch'])).catch(() => {});
    ext.storage.onChanged.addListener((changes) => readKeys(changes?.['dm.catch']?.newValue));
  } catch {
    // No storage here; the keys stay as they ship.
  }
  document.addEventListener('click', (event) => {
    if (!(event.target instanceof Element)) return;
    const keep = held(event, keys.keep);
    const take = !keep && keys.take !== '' && event.isTrusted && held(event, keys.take);
    if (!keep && !take) return;
    const link = event.target.closest('a[href]');
    if (!link) return;
    // The property, not the attribute: resolved against the page.
    try {
      ext.runtime.sendMessage({ type: keep ? 'dm.skip' : 'dm.take', src: String(link.href), frame: '' })
        .catch(() => {});
    } catch {
      // The extension was updated or removed under this page.
    }
  }, true);

  // The background's 'dm.links': the address of every link on the page, or
  // of each one the selection holds. Nothing else is read.
  ext.runtime.onMessage.addListener((message, _sender, reply) => {
    if (!message || message.type !== 'dm.links') return false;
    const selected = message.selected === true;
    const selection = selected ? window.getSelection() : null;
    const selectionText = selection ? String(selection) : '';
    const links = [];
    for (const node of document.querySelectorAll('a[href]')) {
      if (selected) {
        // Inside the selection, or partly inside with its words in the
        // selected text: a link that only touches its edge was not selected.
        if (!selection) continue;
        const whole = selection.containsNode(node, false);
        const words = (node.textContent ?? '').trim();
        const partly = words !== '' && selection.containsNode(node, true)
          && selectionText.includes(words);
        if (!(whole || partly)) continue;
      }
      if (typeof node.href === 'string') links.push(node.href);
    }
    reply({ links });
    return false;
  });

  (async () => {
    const dm = await import(ext.runtime.getURL('pkg/dm_catcher.js'));
    await dm.default({ module_or_path: ext.runtime.getURL('pkg/dm_catcher_bg.wasm') });
    dm.content_start(ext);
  })().catch((e) => console.debug('dm:', e));
})();

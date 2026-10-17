// The page script. Two small things are plain JavaScript, registered before
// anything loads, because they must work on a page whose security policy
// forbids WebAssembly: Alt-click (a link clicked with Alt held is left to
// the browser) and the page's links for "Download all/selected links".
// The "Download video" panel is written in Rust (browser/src/content.rs),
// compiled to WebAssembly; a content script cannot be a module, so this
// loads it. Message names are those in browser/src/decide.rs.
(() => {
  const ext = globalThis.browser ?? globalThis.chrome;
  if (globalThis.__dmPage) return;
  globalThis.__dmPage = true;

  // Alt-click: the background is told before the download starts.
  // Capturing, so a page that stops the click on its way up cannot hide the
  // Alt. Not held to isTrusted: all a made-up Alt+click can do is leave a
  // download with the browser, as it would be with no dm.
  document.addEventListener('click', (event) => {
    if (!event.altKey || !(event.target instanceof Element)) return;
    const link = event.target.closest('a[href]');
    if (!link) return;
    // The property, not the attribute: resolved against the page.
    try {
      ext.runtime.sendMessage({ type: 'dm.skip', src: String(link.href), frame: '' })
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

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
  // The app's master switch and the popup's "Catch downloads": either
  // off, a magnet link is the browser's.
  let rulesOn = true;
  let popupOn = true;
  const catchingNow = () => rulesOn && popupOn;
  const held = (event, key) => (key === 'alt' && event.altKey)
    || (key === 'shift' && event.shiftKey)
    || (key === 'ctrl' && (event.ctrlKey || event.metaKey));
  const readKeys = (rules) => {
    if (!rules || typeof rules !== 'object') return;
    if (typeof rules.keepKey === 'string') keys.keep = rules.keepKey;
    if (typeof rules.takeKey === 'string') keys.take = rules.takeKey;
    if (typeof rules.enabled === 'boolean') rulesOn = rules.enabled;
  };
  try {
    ext.storage.local.get(['dm.catch', 'dm.catching']).then((got) => {
      readKeys(got?.['dm.catch']);
      popupOn = got?.['dm.catching'] !== false;
    }).catch(() => {});
    ext.storage.onChanged.addListener((changes) => {
      readKeys(changes?.['dm.catch']?.newValue);
      if (changes && 'dm.catching' in changes) popupOn = changes['dm.catching'].newValue !== false;
    });
  } catch {
    // No storage here; the keys stay as they ship.
  }
  // A private window's clicks are its own: dm takes nothing from one.
  const privateTab = (() => {
    try { return ext.extension?.inIncognitoContext === true; } catch { return false; }
  })();
  document.addEventListener('click', (event) => {
    if (!(event.target instanceof Element)) return;
    // A magnet link is the app's, as a clicked .torrent is: it went to
    // whatever the system opens magnets with, never dm on macOS or
    // Windows. The background gives it back to the browser when the app
    // cannot take it. Not with the keep key held, nor in a private window.
    const magnet = event.target.closest('a[href]');
    if (magnet && event.button === 0 && catchingNow() && !privateTab && !held(event, keys.keep)
        && /^magnet:\?/i.test(String(magnet.href))) {
      event.preventDefault();
      try {
        // Opened by the page itself where dm does not take it, so the
        // browser's prompt names the site, as it would without dm, not the
        // extension (torrent check 4).
        ext.runtime.sendMessage({ type: 'dm.magnet', src: String(magnet.href), frame: '' })
          .then((answer) => { if (!answer?.ok) window.location.href = magnet.href; })
          .catch(() => { window.location.href = magnet.href; });
      } catch {
        window.location.href = magnet.href;
      }
      return;
    }
    const keep = held(event, keys.keep);
    const take = !keep && !privateTab && keys.take !== '' && event.isTrusted && held(event, keys.take);
    if (!keep && !take) return;
    const link = event.target.closest('a[href]');
    if (!link) return;
    // The take key hands the link to dm as its right-click menu does, so
    // the browser does nothing with it: a link that is not a download was
    // opened in a new window instead.
    if (take) event.preventDefault();
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

  // The panel's WebAssembly is loaded only once the page shows media: it
  // is half a megabyte, and every page and every frame (an advert's, a
  // widget's) loaded and compiled it whether or not it ever held a player.
  // Looked for in the document as it changes (at most once a second), under
  // the pointer inside components that keep their player in a shadow root,
  // and on a play the page starts.
  let loading = false;
  // Where the pointer last was, and what last played, handed over once the
  // panel has loaded: the move or the play that loaded it is over by then,
  // and a pointer resting on a player sends no other.
  let point = null;
  let played = null;
  const load = () => {
    if (loading) return;
    loading = true;
    stop();
    (async () => {
      const dm = await import(ext.runtime.getURL('pkg/dm_catcher.js'));
      await dm.default({ module_or_path: ext.runtime.getURL('pkg/dm_catcher_bg.wasm') });
      dm.content_start(ext);
      if (played) dm.content_played(played);
      else if (point) dm.content_pointer(point.x, point.y);
    })().catch((e) => console.debug('dm:', e));
  };
  const hasMedia = (root) => {
    try { return root.querySelector('video, audio') !== null; } catch { return false; }
  };
  // Media under a point inside components, following shadow roots four
  // deep as the panel's own hit test does: a player inside a component
  // inside another was seen only as the outer one.
  const mediaUnder = (root, x, y, depth) => {
    let stack = [];
    try { stack = root.elementsFromPoint(x, y); } catch { return false; }
    return stack.some((e) => e.shadowRoot && (hasMedia(e.shadowRoot)
      || (depth < 4 && mediaUnder(e.shadowRoot, x, y, depth + 1))));
  };
  let pending = false;
  const look = () => {
    if (loading || pending) return;
    pending = true;
    setTimeout(() => {
      pending = false;
      if (!loading && hasMedia(document)) load();
    }, 1000);
  };
  // At most twice a second, and again at the end of each half second for
  // where the pointer went meanwhile: one that entered a player just after
  // a look elsewhere was not looked at again.
  let lastLook = 0;
  let later = null;
  const lookAtPoint = () => {
    later = null;
    lastLook = Date.now();
    if (!loading && point && mediaUnder(document, point.x, point.y, 0)) load();
  };
  const onPointer = (event) => {
    point = { x: event.clientX, y: event.clientY };
    const wait = 500 - (Date.now() - lastLook);
    if (wait <= 0) lookAtPoint();
    else if (!later) later = setTimeout(lookAtPoint, wait);
  };
  const onPlay = (event) => {
    if (event.target instanceof Element) played = event.target;
    load();
  };
  const observer = new MutationObserver(look);
  const stop = () => {
    if (later) clearTimeout(later);
    observer.disconnect();
    document.removeEventListener('mousemove', onPointer, true);
    document.removeEventListener('play', onPlay, true);
  };
  if (hasMedia(document)) {
    load();
  } else {
    observer.observe(document.documentElement ?? document, { childList: true, subtree: true });
    document.addEventListener('mousemove', onPointer, { capture: true, passive: true });
    document.addEventListener('play', onPlay, true);
  }
})();

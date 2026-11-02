// The page script. Two small things are plain JavaScript, registered before
// anything loads, because they must work on a page whose security policy
// forbids WebAssembly: a click with a key held (Alt leaves a download with
// the browser, Shift hands it to dm, as the app's rules name them) and the
// page's links for "Download all/selected links".
// The "Download video" panel is written in Rust (browser/src/content.rs),
// compiled to WebAssembly; a content script cannot be a module, so the
// background puts it in when the page shows media. Message names are those in browser/src/decide.rs.
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

  // What document.js saw a video player ask for (on the sites it is
  // registered for), told to the background so the panel's list can offer
  // the picture and sound together. Only from this page, only an address.
  window.addEventListener('message', (event) => {
    const data = event.data;
    if (event.source !== window || data?.source !== 'dm-media-seen' || typeof data.url !== 'string') return;
    try {
      ext.runtime.sendMessage({ type: 'dm.media-seen', url: data.url }).catch(() => {});
    } catch { /* the extension was updated under this page */ }
  });

  // On a site the app's rules name, the requests whose JSON answers hold the
  // streams its page builds: the page script is told which, and what it reads
  // comes back here to the background, which finds the streams (in Rust, only
  // where the rules for the site name that request). Only from this page.
  try {
    ext.runtime.sendMessage({ type: 'dm.page-rules' }).then((answer) => {
      if (answer?.ok && Array.isArray(answer.urls) && answer.urls.length) {
        window.postMessage({ source: 'dm-page-rules', urls: answer.urls }, location.origin);
      }
    }).catch(() => {});
  } catch { /* the extension was updated under this page */ }
  window.addEventListener('message', (event) => {
    const data = event.data;
    if (event.source !== window || data?.source !== 'dm-page-answer'
      || typeof data.url !== 'string' || typeof data.body !== 'string') return;
    try {
      ext.runtime.sendMessage({ type: 'dm.page-answer', url: data.url, body: data.body }).catch(() => {});
    } catch { /* the extension was updated under this page */ }
  });

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
  // The file types the app catches (empty: every type), and the ones a
  // browser shows in the tab and never downloads, so a click on a link to
  // one starts no download for the downloads list to catch.
  let wanted = null;
  const shownInTab = new Set(['pdf', 'mp4', 'm4v', 'webm', 'ogv', 'mov', 'mp3', 'm4a', 'wav',
    'ogg', 'opus', 'flac', 'aac']);
  const inlineKind = (link) => {
    let url;
    try { url = new URL(link.href); } catch { return false; }
    if (url.protocol !== 'http:' && url.protocol !== 'https:') return false;
    const name = decodeURIComponent(url.pathname).split('/').pop() ?? '';
    const dot = name.lastIndexOf('.');
    if (dot < 1) return false;
    const kind = name.slice(dot + 1).toLowerCase();
    return shownInTab.has(kind) && (wanted === null || wanted.size === 0 || wanted.has(kind));
  };
  const catchingNow = () => rulesOn && popupOn;
  const held = (event, key) => (key === 'alt' && event.altKey)
    || (key === 'shift' && event.shiftKey)
    || (key === 'ctrl' && (event.ctrlKey || event.metaKey));
  const readKeys = (rules) => {
    if (!rules || typeof rules !== 'object') return;
    if (typeof rules.keepKey === 'string') keys.keep = rules.keepKey;
    if (typeof rules.takeKey === 'string') keys.take = rules.takeKey;
    if (typeof rules.enabled === 'boolean') rulesOn = rules.enabled;
    if (Array.isArray(rules.extensions)) {
      wanted = new Set(rules.extensions.map((e) => String(e).trim().replace(/^\./, '').toLowerCase()));
    }
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
    // cannot take it. Not with the keep key held, nor in a private window,
    // nor for a click a page's script made up: a person's click only, or
    // any page could hand dm a torrent to fetch.
    const magnet = event.target.closest('a[href]');
    if (magnet && event.isTrusted && event.button === 0 && catchingNow() && !privateTab && !held(event, keys.keep)
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
    // A plain click on a link to a file the browser would show in the tab:
    // the app takes it where its rules do, and the link opens as it would
    // have where they do not. A person's click only, with no key held (a
    // key opens a tab or a window, or is the keep and take keys' own).
    if (event.isTrusted && event.button === 0 && catchingNow() && !privateTab
        && !event.altKey && !event.ctrlKey && !event.metaKey && !event.shiftKey) {
      const shown = event.target.closest('a[href]');
      if (shown && !shown.hasAttribute('download') && inlineKind(shown)) {
        const href = String(shown.href);
        const newTab = shown.target === '_blank';
        const open = () => { if (newTab) window.open(href, '_blank', 'noopener'); else window.location.href = href; };
        event.preventDefault();
        try {
          ext.runtime.sendMessage({ type: 'dm.inline', src: href, frame: '' })
            .then((answer) => { if (!answer?.ok) open(); })
            .catch(open);
        } catch {
          open();
        }
        return;
      }
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
  // Where the panel's WebAssembly cannot load (a page whose security policy forbids it:
  // Reddit's), a plain-JavaScript panel does the same job with the same messages to the
  // background: a button over the video, the list of what it comes in, and a line to
  // say it was sent. Only a click the person made counts, as in the full panel.
  const lite = (() => {
    let host = null;
    let parts = null;
    let current = null;
    let busy = false;
    let hideTimer = null;
    let lastScan = 0;
    const STYLE = ':host{all:initial;position:fixed;z-index:2147483647;display:none}'
      + '.bar{display:flex;font:600 13px system-ui,sans-serif;color:#fff;background:rgba(32,32,32,.94);'
      + 'border:1px solid rgba(255,255,255,.25);border-radius:6px;overflow:hidden}'
      + 'button{all:unset;cursor:pointer;padding:6px 10px;font:inherit;color:inherit}'
      + 'button:hover,button:focus-visible{background:#0a64d8}'
      + '.x{padding:6px 8px;font-weight:400;border-left:1px solid rgba(255,255,255,.25)}'
      + '.list{margin-top:4px;background:rgba(32,32,32,.96);border:1px solid rgba(255,255,255,.25);'
      + 'border-radius:6px;overflow:hidden auto;max-height:260px;max-width:320px}'
      + '.list button{display:block;width:100%;box-sizing:border-box;padding:8px 12px;font-weight:400;'
      + 'white-space:normal;overflow-wrap:anywhere;border-top:1px solid rgba(255,255,255,.15)}'
      + '.say{margin-top:4px;padding:6px 10px;background:rgba(32,32,32,.94);border-radius:6px;color:#ddd;'
      + 'font:12px system-ui,sans-serif;max-width:320px;overflow-wrap:anywhere}'
      + '[hidden]{display:none!important}';
    const build = () => {
      host = document.createElement('dm-panel-lite');
      const root = host.attachShadow({ mode: 'open' });
      const style = document.createElement('style');
      style.textContent = STYLE;
      const bar = document.createElement('div');
      bar.className = 'bar';
      const go = document.createElement('button');
      go.textContent = 'Download video';
      const close = document.createElement('button');
      close.className = 'x';
      close.textContent = '×';
      close.setAttribute('aria-label', 'Hide');
      bar.append(go, close);
      const list = document.createElement('div');
      list.className = 'list';
      list.hidden = true;
      const say = document.createElement('div');
      say.className = 'say';
      say.hidden = true;
      root.append(style, bar, list, say);
      parts = { go, close, list, say };
      // Reset to a plain box; the browser's own popover rules would centre and frame it.
      // Said outright: the page's own rules (a hidden or see-through <html>) reach it by inheritance.
      host.style.cssText = 'position:fixed;inset:auto;margin:0;padding:0;border:0;background:none;'
        + 'overflow:visible;width:auto;height:auto;display:none;color:inherit;visibility:visible;opacity:1;'
        + 'pointer-events:auto;transform:none;filter:none;clip-path:none;';
      document.documentElement.appendChild(host);
      // In the top layer, where a page's own player (Reddit's) cannot be drawn over it.
      try { host.setAttribute('popover', 'manual'); host.showPopover(); } catch (e) { /* an older browser: z-index it is */ }
      host.addEventListener('mouseenter', () => clearTimeout(hideTimer));
      go.addEventListener('click', (event) => { if (event.isTrusted) ask(); });
      close.addEventListener('click', (event) => { if (event.isTrusted) hide(); });
    };
    const hide = () => {
      if (host) host.style.display = 'none';
      if (parts) { parts.list.hidden = true; parts.say.hidden = true; }
      current = null;
    };
    // Left up while it is working or its list is open; a message it gave ends it for sure.
    const hideSoon = (ms, last) => {
      clearTimeout(hideTimer);
      hideTimer = setTimeout(() => {
        if (last) { hide(); return; }
        if (busy || (parts && !parts.list.hidden) || host?.matches(':hover')) { hideSoon(ms); return; }
        hide();
      }, ms);
    };
    const said = (text) => {
      parts.say.textContent = text || '';
      parts.say.hidden = !text;
    };
    const send = (message) => ext.runtime.sendMessage({ ...message, frame: location.href });
    const srcOf = () => (current && (current.currentSrc || current.src)) || '';
    const finish = async (extra) => {
      parts.list.hidden = true;
      said('Sending to dm…');
      let reply;
      try { reply = await send({ type: 'dm.media', src: extra.url || srcOf(), ...extra.fields }); } catch (e) { reply = { ok: false, said: 'dm was updated: reload this page to use it' }; }
      said(reply?.ok ? 'Sent to dm. ' + (extra.label ? extra.label + ': ' : '') + (reply.said || '') : (reply?.said || 'dm could not take it'));
      hideSoon(2500, true);
    };
    const choose = (choice) => finish({
      url: choice.url || '',
      label: choice.label || '',
      fields: {
        ...(choice.quality ? { quality: choice.quality } : {}),
        ...(choice.label ? { label: choice.label } : {}),
        ...(choice.sound ? { sound: choice.sound } : {}),
      },
    });
    const ask = async () => {
      if (busy || !current) return;
      busy = true;
      said('Reading qualities…');
      try {
        const reply = await send({ type: 'dm.formats', src: srcOf() });
        said('');
        if (!reply?.ok) { said(reply?.said || 'dm did not answer'); hideSoon(5000, true); return; }
        const choices = Array.isArray(reply.choices) ? reply.choices : [];
        if (choices.length === 0) await finish({ fields: {} });
        else if (choices.length === 1) await choose(choices[0]);
        else {
          parts.list.textContent = '';
          for (const choice of choices) {
            const item = document.createElement('button');
            item.textContent = choice.line || choice.label || 'Download';
            item.addEventListener('click', (event) => { if (event.isTrusted) choose(choice); });
            parts.list.appendChild(item);
          }
          parts.list.hidden = false;
        }
      } catch (e) {
        said('dm did not answer. Try again, or right-click the video and choose Download media with dm.');
        hideSoon(5000, true);
      } finally { busy = false; }
    };
    // The media under a point, followed into open shadow roots as the full panel's hit test is.
    const mediaIn = (root, x, y, depth) => {
      let stack = [];
      try { stack = root.elementsFromPoint(x, y); } catch { return null; }
      const direct = stack.find((e) => e.tagName === 'VIDEO' || e.tagName === 'AUDIO');
      if (direct) return direct;
      if (depth < 4) {
        for (const e of stack) {
          if (e.shadowRoot) {
            const found = mediaIn(e.shadowRoot, x, y, depth + 1);
            if (found) return found;
          }
        }
      }
      return null;
    };
    const place = (video) => {
      const r = video.getBoundingClientRect();
      if (r.width < 150 || r.height < 100) return;
      if (!host) build();
      current = video;
      host.style.left = Math.max(0, Math.min(window.innerWidth - 240, r.right - 240 - 8)) + 'px';
      host.style.top = Math.max(0, r.top + 8) + 'px';
      host.style.display = 'block';
      hideSoon(4000);
    };
    const onMove = (event) => {
      if (Date.now() - lastScan < 150) return;
      lastScan = Date.now();
      if (host && event.composedPath().includes(host)) { clearTimeout(hideTimer); return; }
      const video = mediaIn(document, event.clientX, event.clientY, 0);
      if (video) place(video);
    };
    return {
      start: () => document.addEventListener('mousemove', onMove, { capture: true, passive: true }),
    };
  })();
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
      // Put into this frame by the background (pkg/panel.js), which
      // leaves the panel's functions here once its WebAssembly is ready.
      const put = await ext.runtime.sendMessage({ type: 'dm.panel-load' });
      if (!put?.ok) throw new Error(put?.said ?? 'the panel was not put in');
      const dm = await globalThis.__dmPanelCode;
      dm.content_start(ext);
      if (played) dm.content_played(played);
      else if (point) dm.content_pointer(point.x, point.y);
    })().catch((e) => { console.debug('dm:', e); lite.start(); });
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

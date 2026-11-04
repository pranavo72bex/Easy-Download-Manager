// Runs in the page's own world (manifest "world": "MAIN"), where a content
// script's isolated world cannot see the player's requests. It only reads:
// the address a video site's player asks for is passed on to content.js,
// which tells the background; the request itself goes on untouched.
// On the sites the app's rules name, the JSON answer of the requests the rules
// list is passed on too (at most 1 MiB of it), because a page that builds its
// own stream puts the address in an answer and not in a request of its own.
// What is a stream in it is decided in Rust (browser/src/decide/answers.rs).
(() => {
  if (globalThis.__dmHook) return;
  globalThis.__dmHook = true;
  // A manifest, a playlist or a whole media file a player asks for, on any site. The pieces of a
  // stream (segments, initialisation slices, byte ranges) are not passed on: they are not what a
  // person comes to save, and a player asks for hundreds. What is a stream is decided in Rust
  // (browser/src/decide/streams.rs); this only keeps the traffic to the extension small.
  const STREAM_URL = /\.(m3u8|mpd|webm|mp4|m4a|aac|f4m)(\?|#|$)/i;
  const PIECE_URL = /\.(ts|m4s|m4f|cmfv|cmfa)(\?|#|$)|[?&](range|bytestart|byterange|sq|segment)=/i;
  const told = new Set();
  const tell = (url) => {
    try {
      const text = typeof url === 'string' ? url : url?.url;
      if (typeof text !== 'string') return;
      const u = new URL(text, location.href);
      if (u.protocol !== 'http:' && u.protocol !== 'https:') return;
      const google = u.hostname === 'googlevideo.com' || u.hostname.endsWith('.googlevideo.com');
      if (google) {
        if (!text.includes('/videoplayback')) return;
      } else {
        const where = u.pathname + u.search;
        if (!STREAM_URL.test(where) || PIECE_URL.test(where) || told.has(u.href) || told.size > 500) return;
        told.add(u.href);
      }
      window.postMessage({ source: 'dm-media-seen', url: u.href }, location.origin);
    } catch { /* an address that is not one is not media */ }
  };
  // A player that feeds a MediaSource plays a blob: address that names nothing. What it declares
  // (the codecs of its picture and its sound) is passed on, so content.js can tell a page with an
  // adaptive player from one with a plain file; the bytes are never read.
  try {
    const sources = new WeakMap();
    const createObjectURL = URL.createObjectURL;
    URL.createObjectURL = function (object) {
      const url = createObjectURL.call(this, object);
      try {
        if (typeof MediaSource !== 'undefined' && object instanceof MediaSource) {
          window.postMessage({ source: 'dm-mediasource', blob: url, types: sources.get(object) || [] }, location.origin);
        }
      } catch { /* the page's own call goes on */ }
      return url;
    };
    if (typeof MediaSource !== 'undefined') {
      const addSourceBuffer = MediaSource.prototype.addSourceBuffer;
      MediaSource.prototype.addSourceBuffer = function (type) {
        try {
          const known = sources.get(this) || [];
          if (known.length < 8) sources.set(this, [...known, String(type).slice(0, 200)]);
        } catch { /* the page's own call goes on */ }
        return addSourceBuffer.call(this, type);
      };
    }
  } catch { /* no MediaSource here: the requests are still read */ }
  // The player's own list of qualities, for the panel's list: asked by
  // the content script with a token, answered at once from the page's
  // player with the id of the video it is playing, so the panel lists
  // them before the app's reading (with sizes) arrives.
  window.addEventListener('message', (event) => {
    if (event.source !== window || event.data?.source !== 'dm-ask-qualities') return;
    let levels = [];
    let id = null;
    try {
      const player = document.querySelector('#movie_player');
      if (player && typeof player.getAvailableQualityLevels === 'function') {
        levels = player.getAvailableQualityLevels() || [];
        id = (typeof player.getVideoData === 'function' && player.getVideoData()?.video_id) || null;
      }
    } catch { levels = []; }
    window.postMessage({ source: 'dm-player-qualities', token: event.data.token, id, levels }, location.origin);
  });
  // The parts of an address that mark a request whose answer is read: told by
  // content.js from the app's rules for this site; empty on every other site.
  let reading = [];
  const MAX_ANSWER = 1024 * 1024;
  let finders = [];
  window.addEventListener('message', (event) => {
    if (event.source !== window || event.data?.source !== 'dm-page-rules') return;
    if (Array.isArray(event.data.urls)) {
      reading = event.data.urls
        .filter((part) => typeof part === 'string' && part)
        .map((part) => part.toLowerCase())
        .slice(0, 64);
    }
    if (Array.isArray(event.data.finders)) {
      finders = event.data.finders.filter((f) => f && typeof f === 'object').slice(0, 16);
    }
    if (Array.isArray(event.data.privateFetches)) {
      for (const spec of event.data.privateFetches.slice(0, 16)) hookPrivateFetch(spec);
    }
  });
  // A property path, read only: dotted, data properties only (nothing is called), at most 32 steps.
  // A leading `*` means the first own property of the object for which the rest of the path
  // exists, which is how React's `__reactFiber$<random>` is reached.
  const MAX_STEPS = 32;
  const own = (object, name) => {
    const d = Object.getOwnPropertyDescriptor(object, name);
    return d && 'value' in d ? d.value : undefined;
  };
  const walk = (object, names, budget) => {
    let at = object;
    for (let i = 0; i < names.length; i++) {
      if (--budget.steps < 0 || Date.now() > budget.until) return undefined;
      if (at === null || (typeof at !== 'object' && typeof at !== 'function')) return undefined;
      if (names[i] === '*') {
        if (i !== 0) return undefined;
        for (const key of Object.getOwnPropertyNames(at)) {
          if (--budget.steps < 0 || Date.now() > budget.until) return undefined;
          const first = own(at, key);
          if (first === undefined) continue;
          const found = walk(first, names.slice(1), budget);
          if (found !== undefined) return found;
        }
        return undefined;
      }
      at = own(at, names[i]);
      if (at === undefined) return undefined;
    }
    return at;
  };
  const pathValue = (object, path) => {
    const names = String(path).split('.').filter(Boolean);
    if (!names.length) return undefined;
    return walk(object, names, { steps: MAX_STEPS * 8, until: Date.now() + 5 });
  };
  // A selector chain, read only, at most 32 steps. Each step starts with `$` and a sign:
  // `$<sel` the closest ancestor, `$>sel` a descendant, `$|sel` a descendant if there is one (else
  // stay), `${sel` stop if `sel` is there, `$}sel` stop if it is not, `$&`/`$+` take this element,
  // `$*sel` take every match below, `$=name` take an attribute's value, `$@` take the text, and a
  // trailing `<` steps to the parent. A chain with no `$` is one selector, matched by the element
  // or an ancestor of it. The result is a list of elements or strings, empty when the chain stops.
  const chain = (start, text) => {
    const source = String(text || '').trim();
    if (!source) return [];
    if (!source.startsWith('$')) {
      try { const hit = start.closest(source); return hit ? [hit] : []; } catch { return []; }
    }
    const steps = source.split('$').slice(1).slice(0, MAX_STEPS);
    let at = start;
    const taken = [];
    try {
      for (const step of steps) {
        let sign = step[0];
        let argument = step.slice(1);
        const up = argument.endsWith('<');
        if (up) argument = argument.slice(0, -1);
        argument = argument.trim();
        if (sign === '<') { at = at.closest(argument); if (!at) return []; }
        else if (sign === '>') { at = at.querySelector(argument); if (!at) return []; }
        else if (sign === '|') { at = (argument && at.querySelector(argument)) || at; }
        else if (sign === '{') { if (at.matches(argument) || at.querySelector(argument)) return []; }
        else if (sign === '}') { if (!at.matches(argument) && !at.querySelector(argument)) return []; }
        else if (sign === '&' || sign === '+') taken.push(at);
        else if (sign === '*') { taken.push(...Array.from(at.querySelectorAll(argument)).slice(0, 64)); return taken; }
        else if (sign === '=') { const value = at.getAttribute(argument); if (value === null) return []; taken.push(value); return taken; }
        else if (sign === '@') { taken.push((at.textContent || '').trim().slice(0, 1000)); return taken; }
        else return [];
        if (up) { at = at.parentElement; if (!at) return []; }
      }
    } catch { return []; }
    return taken.length ? taken : [at];
  };
  // The ids of the players in a feed, read as they appear: a finder's `container` is watched for
  // new players (feeds add them as a person scrolls), and each one's id is kept, because a
  // framework may drop the property a path reads once the player has been on screen a while.
  const known = new WeakMap();
  const idOf = (element) => {
    for (const finder of finders) {
      if (!chain(element, finder.player).length) continue;
      let value;
      if (finder.id_path) value = pathValue(element, finder.id_path);
      if (value === undefined && finder.id_attr) value = element.getAttribute(finder.id_attr) ?? undefined;
      if (value === undefined && finder.id_from_url) {
        const hit = new RegExp(finder.id_from_url).exec(location.href);
        value = hit ? hit[1] : undefined;
      }
      if (typeof value === 'string' || typeof value === 'number') return String(value).slice(0, 200);
    }
    return '';
  };
  const watching = new WeakSet();
  const readPlayers = (root) => {
    try {
      for (const el of Array.from(root.querySelectorAll('video, [data-dm-player], div')).slice(0, 300)) {
        if (known.has(el)) continue;
        const id = idOf(el);
        if (id) known.set(el, id);
      }
    } catch { /* a page that cannot be read is not */ }
  };
  let pending = 0;
  const watchContainers = () => {
    for (const finder of finders) {
      if (!finder.container) continue;
      let hits = [];
      try { hits = chain(document.documentElement, finder.container).filter((e) => e instanceof Element); } catch { hits = []; }
      for (const box of hits.slice(0, 8)) {
        if (watching.has(box)) continue;
        watching.add(box);
        new MutationObserver(() => {
          if (pending) return;
          pending = setTimeout(() => { pending = 0; readPlayers(box); }, 400);
        }).observe(box, { childList: true, subtree: true });
        readPlayers(box);
      }
    }
  };
  window.addEventListener('message', (event) => {
    if (event.source === window && event.data?.source === 'dm-page-rules' && finders.length) {
      setTimeout(watchContainers, 0);
    }
  });
  // Which video a player is: asked by content.js with the element marked, answered at once.
  window.addEventListener('message', (event) => {
    if (event.source !== window || event.data?.source !== 'dm-find-id') return;
    const token = String(event.data.token || '').replace(/[^a-z0-9]/gi, '').slice(0, 40);
    let id = '';
    try {
      const element = token ? document.querySelector('[data-dm-find="' + token + '"]') : null;
      if (element) id = idOf(element) || known.get(element) || '';
    } catch { id = ''; }
    window.postMessage({ source: 'dm-player-id', token: event.data.token, id }, location.origin);
  });
  const watched = (asked) => {
    if (!reading.length) return null;
    try {
      const text = typeof asked === 'string' ? asked : asked?.url ?? String(asked);
      const href = new URL(text, location.href).href;
      const lower = href.toLowerCase();
      return reading.some((part) => lower.includes(part)) ? href : null;
    } catch { return null; }
  };
  // On every site, an answer of the page's own that names media (a manifest, a playlist,
  // an .mp4 address) is passed on to content.js, which keeps it in memory for the
  // panel's list of qualities and sends it nowhere: a feed loads each video's
  // renditions through its API after the page, so the page's markup does not hold them.
  const MEDIA_ANSWER = 2 * 1024 * 1024;
  const MEDIA_WORDS = /\.m3u8|\.mpd|<MPD|#EXTM3U|\.mp4|\.webm/;
  const TEXTUAL = /json|xml|text|mpegurl|dash/i;
  const keep = (body) => {
    if (typeof body !== 'string' || !body || body.length > MEDIA_ANSWER || !MEDIA_WORDS.test(body)) return;
    window.postMessage({ source: 'dm-page-body', body }, location.origin);
  };
  // Whether the first bytes are text and not a media file: no NUL and no control byte but the
  // whitespace ones, so an API that answers JSON as application/octet-stream is read and a video
  // chunk is not.
  const looksText = (bytes) => {
    const head = bytes.subarray(0, 64);
    if (!head.length) return false;
    for (const b of head) if (b === 0 || b < 9 || (b > 13 && b < 32)) return false;
    return true;
  };
  const decode = (bytes) => new TextDecoder('utf-8', { fatal: false }).decode(bytes);
  const tellAnswer = (href, body) => {
    if (typeof body !== 'string' || !body || body.length > MAX_ANSWER) return;
    window.postMessage({ source: 'dm-page-answer', url: href, body }, location.origin);
  };
  const jsonLike = (text) => /^\s*[{[]/.test(text);
  // What the page itself read of an answer, as text, passed on: to the panel's list when it names
  // media, and to the app's reading when the rules name the request. Nothing is fetched twice.
  const passOn = (response, text) => {
    try {
      if (typeof text !== 'string' || !text || !response?.ok) return;
      const type = (response.headers.get('content-type') || '').toLowerCase();
      if (TEXTUAL.test(type) || !type || type.includes('octet-stream') || jsonLike(text)) keep(text);
      const href = watched(response.url);
      if (href && (type.includes('json') || jsonLike(text))) tellAnswer(href, text);
    } catch { /* an answer that cannot be passed on is not */ }
  };
  const textOfBytes = (bytes) => (bytes.length <= MEDIA_ANSWER && looksText(bytes) ? decode(bytes) : null);
  // The one half of a tee() the page did not take, read to the cap and decoded if it is text.
  const readStream = async (response, stream) => {
    try {
      const reader = stream.getReader();
      const parts = [];
      let total = 0;
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        if (parts.length === 0 && !looksText(value)) { reader.cancel().catch(() => {}); return; }
        total += value.length;
        if (total > MEDIA_ANSWER) { reader.cancel().catch(() => {}); return; }
        parts.push(value);
      }
      const bytes = new Uint8Array(total);
      let at = 0;
      for (const part of parts) { bytes.set(part, at); at += part.length; }
      passOn(response, decode(bytes));
    } catch { /* an answer that cannot be read is not read */ }
  };
  // The Response reading methods are wrapped to pass the result on after the page's own promise has
  // resolved with it; what the page gets is what it would have got.
  try {
    const proto = Response.prototype;
    const wrap = (name, toText) => {
      const original = proto[name];
      if (typeof original !== 'function') return;
      proto[name] = function (...args) {
        const pending = original.apply(this, args);
        try {
          const response = this;
          pending.then(async (result) => {
            try { passOn(response, await toText(result)); } catch { /* not read */ }
          }, () => {});
        } catch { /* the page's own call goes on */ }
        return pending;
      };
    };
    wrap('text', (text) => (text.length <= MEDIA_ANSWER ? text : null));
    wrap('json', (value) => {
      const text = JSON.stringify(value);
      return typeof text === 'string' && text.length <= MEDIA_ANSWER ? text : null;
    });
    wrap('arrayBuffer', (buffer) => textOfBytes(new Uint8Array(buffer)));
    wrap('blob', async (blob) => (blob.size <= MEDIA_ANSWER ? textOfBytes(new Uint8Array(await blob.arrayBuffer())) : null));
    wrap('formData', (form) => {
      const parts = [];
      form.forEach((value) => { if (typeof value === 'string') parts.push(value); });
      return parts.join('\n');
    });
    // A page that takes `body` gets one half of a tee(); the other half is read. Only an answer
    // that may be text is split, so a media chunk a player streams is never held up by it.
    const bodyOf = Object.getOwnPropertyDescriptor(proto, 'body');
    if (bodyOf?.get) {
      const given = new WeakMap();
      Object.defineProperty(proto, 'body', {
        ...bodyOf,
        get() {
          const stream = bodyOf.get.call(this);
          if (!stream) return stream;
          if (given.has(this)) return given.get(this);
          try {
            const type = (this.headers.get('content-type') || '').toLowerCase();
            const size = Number(this.headers.get('content-length'));
            const maybeText = TEXTUAL.test(type) || !type || type.includes('octet-stream');
            if (!this.ok || !maybeText || size > MEDIA_ANSWER || stream.locked) return stream;
            const [forPage, forDm] = stream.tee();
            given.set(this, forPage);
            readStream(this, forDm);
            return forPage;
          } catch { return stream; }
        },
      });
    }
  } catch { /* no Response here: the fetch hook below still reads the address */ }
  // A magnet link a page opens with script (window.open, or a link it makes and clicks) rather than
  // as a link a person clicked: passed to content.js, which takes it for dm only while the person's own
  // click is still active (the page's user activation), as a clicked link is. The browser's own way is
  // not left: where dm cannot take it, content.js opens it as the page meant to.
  const tellMagnet = (target) => {
    try {
      const text = String(target?.href ?? target);
      if (!/^magnet:\?/i.test(text) || !navigator.userActivation?.isActive) return false;
      // The page's own way, when dm could not take it: content.js marks the page with the time, and
      // a magnet opened within three seconds of it is left alone (the exact address is not compared:
      // the browser may write it a little differently).
      const marked = Number(document.documentElement?.getAttribute('data-dm-magnet-pass'));
      if (marked && Date.now() - marked < 3000) return false;
      window.postMessage({ source: 'dm-magnet', url: text }, location.origin);
      return true;
    } catch { return false; }
  };
  const opened_ = window.open;
  window.open = function (url, ...rest) {
    if (tellMagnet(url)) return null;
    return opened_.call(this, url, ...rest);
  };
  const clicked_ = HTMLAnchorElement.prototype.click;
  HTMLAnchorElement.prototype.click = function () {
    if (tellMagnet(this)) return undefined;
    return clicked_.call(this);
  };
  // A page that sets location to a magnet link (a button, or a link's own script): the browser
  // reports it, and it can be cancelled, so dm can take it instead.
  try {
    window.navigation?.addEventListener('navigate', (event) => {
      const to = String(event.destination?.url ?? '');
      if (event.cancelable && /^magnet:\?/i.test(to) && tellMagnet(to)) event.preventDefault();
    });
  } catch { /* no Navigation API here: the clicked link and the script's open are still taken */ }
  const fetched = window.fetch;
  window.fetch = function (...args) {
    tell(args[0]);
    return fetched.apply(this, args);
  };
  // A site's own copy of `fetch` (a property path the app's rules name, as
  // `<global>|<path>|<name>`) is read as the global one is: the address it asks for is told.
  function hookPrivateFetch(spec) {
    try {
      const [name, path, method] = String(spec).split('|');
      const root = globalThis[name.trim()];
      if (!root || typeof root !== 'object' && typeof root !== 'function') return;
      const owner = path.trim() ? pathValue(root, path.trim()) : root;
      if (!owner || typeof owner[method] !== 'function' || owner[method].__dmHooked) return;
      const original = owner[method];
      const hooked = function (...args) {
        tell(args[0]);
        return original.apply(this, args);
      };
      hooked.__dmHooked = true;
      owner[method] = hooked;
    } catch { /* a site's own function that cannot be wrapped is left alone */ }
  }
  const opened = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function (method, url, ...rest) {
    tell(url);
    const href = watched(url);
    this.addEventListener('load', async () => {
      try {
        if (this.status < 200 || this.status > 299) return;
        const type = (this.getResponseHeader('content-type') || '').toLowerCase();
        let text = null;
        switch (this.responseType) {
          case '':
          case 'text':
            text = this.responseText;
            break;
          case 'json':
            text = JSON.stringify(this.response);
            break;
          case 'arraybuffer':
            text = this.response ? textOfBytes(new Uint8Array(this.response)) : null;
            break;
          case 'blob':
            text = this.response && this.response.size <= MEDIA_ANSWER
              ? textOfBytes(new Uint8Array(await this.response.arrayBuffer())) : null;
            break;
          default:
            return;
        }
        if (typeof text !== 'string' || !text || text.length > MEDIA_ANSWER) return;
        if (TEXTUAL.test(type) || !type || type.includes('octet-stream') || jsonLike(text)) keep(text);
        if (href && (type.includes('json') || jsonLike(text))) tellAnswer(href, text);
      } catch { /* an answer that cannot be read is not kept */ }
    });
    return opened.call(this, method, url, ...rest);
  };
})();

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
  window.addEventListener('message', (event) => {
    if (event.source !== window || event.data?.source !== 'dm-page-rules'
      || !Array.isArray(event.data.urls)) return;
    reading = event.data.urls
      .filter((part) => typeof part === 'string' && part)
      .map((part) => part.toLowerCase())
      .slice(0, 64);
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
  const keepFetched = async (response) => {
    try {
      if (!response?.ok || !TEXTUAL.test(response.headers.get('content-type') || '')) return;
      if (Number(response.headers.get('content-length')) > MEDIA_ANSWER) return;
      keep(await response.clone().text());
    } catch { /* an answer that cannot be read is not kept */ }
  };
  const tellAnswer = (href, body) => {
    if (typeof body !== 'string' || !body || body.length > MAX_ANSWER) return;
    window.postMessage({ source: 'dm-page-answer', url: href, body }, location.origin);
  };
  // A fetch answer is read from a copy, in pieces, and given up on past the cap.
  const readFetched = async (href, response) => {
    try {
      if (!response?.ok || !(response.headers.get('content-type') || '').toLowerCase().includes('json')) return;
      if (Number(response.headers.get('content-length')) > MAX_ANSWER) return;
      const reader = response.clone().body?.getReader();
      if (!reader) return;
      const parts = [];
      let total = 0;
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        total += value.length;
        if (total > MAX_ANSWER) { reader.cancel().catch(() => {}); return; }
        parts.push(value);
      }
      tellAnswer(href, await new Blob(parts).text());
    } catch { /* an answer that cannot be read is not read */ }
  };
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
    const pending = fetched.apply(this, args);
    const href = watched(args[0]);
    if (href) pending.then((response) => readFetched(href, response), () => {});
    pending.then(keepFetched, () => {});
    return pending;
  };
  const opened = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function (method, url, ...rest) {
    tell(url);
    this.addEventListener('load', () => {
      try {
        if (this.status < 200 || this.status > 299) return;
        if (this.responseType !== '' && this.responseType !== 'text') return;
        if (!TEXTUAL.test(this.getResponseHeader('content-type') || '')) return;
        keep(this.responseText);
      } catch { /* an answer that cannot be read is not kept */ }
    });
    const href = watched(url);
    if (href) {
      this.addEventListener('load', () => {
        try {
          if (this.status < 200 || this.status > 299) return;
          if (this.responseType !== '' && this.responseType !== 'text') return;
          if (!(this.getResponseHeader('content-type') || '').toLowerCase().includes('json')) return;
          tellAnswer(href, this.responseText);
        } catch { /* an answer that cannot be read is not read */ }
      });
    }
    return opened.call(this, method, url, ...rest);
  };
})();

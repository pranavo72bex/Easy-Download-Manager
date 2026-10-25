// Runs in the page's own world (manifest "world": "MAIN"), where a content
// script's isolated world cannot see the player's requests. It only reads:
// the address a video site's player asks for is passed on to content.js,
// which tells the background; the request itself goes on untouched.
(() => {
  if (globalThis.__dmHook) return;
  globalThis.__dmHook = true;
  const tell = (url) => {
    try {
      const text = typeof url === 'string' ? url : url?.url;
      if (typeof text !== 'string' || !text.includes('/videoplayback')) return;
      const u = new URL(text, location.href);
      if (u.hostname !== 'googlevideo.com' && !u.hostname.endsWith('.googlevideo.com')) return;
      window.postMessage({ source: 'dm-media-seen', url: u.href }, location.origin);
    } catch { /* an address that is not one is not media */ }
  };
  const fetched = window.fetch;
  window.fetch = function (...args) {
    tell(args[0]);
    return fetched.apply(this, args);
  };
  const opened = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function (method, url, ...rest) {
    tell(url);
    return opened.call(this, method, url, ...rest);
  };
})();

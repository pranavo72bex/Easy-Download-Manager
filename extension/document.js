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

"use strict";
/* Integración con el sistema: pantalla encendida mientras se reproduce (Wake Lock) y controles/título
   en la pantalla de bloqueo y teclas multimedia (Media Session). Todo opcional: sin soporte, no hace nada. */
function createPlatform({ video, actions }) {
  let lock = null, lastPos = 0;
  const ms = "mediaSession" in navigator ? navigator.mediaSession : null;

  async function acquire() {
    if (lock || document.visibilityState !== "visible" || !navigator.wakeLock) return;
    try {
      lock = await navigator.wakeLock.request("screen");
      lock.addEventListener("release", () => { lock = null; });
    } catch { lock = null; }
  }
  function release() {
    lock?.release?.().catch(() => {});
    lock = null;
  }
  video.addEventListener("play", acquire);
  for (const ev of ["pause", "ended"]) video.addEventListener(ev, release);
  document.addEventListener("visibilitychange", () => { if (!video.paused) acquire(); });

  const handler = (name, fn) => { try { ms.setActionHandler(name, fn); } catch {} };
  if (ms) {
    handler("play", () => video.play().catch(() => {}));
    handler("pause", () => video.pause());
    handler("seekbackward", () => actions.skip(-10));
    handler("seekforward", () => actions.skip(10));
    video.addEventListener("timeupdate", () => {
      const now = Date.now();
      if (now - lastPos < 1000 || !isFinite(video.duration)) return;
      lastPos = now;
      try { ms.setPositionState({ duration: video.duration, position: Math.min(video.currentTime, video.duration), playbackRate: video.playbackRate || 1 }); } catch {}
    });
  }

  return {
    open(title, next) {
      if (!ms) return;
      ms.metadata = new MediaMetadata({ title, artist: "GordoFlix" });
      handler("nexttrack", next || null);
    },
    close() {
      release();
      if (ms) { ms.metadata = null; handler("nexttrack", null); }
    },
  };
}

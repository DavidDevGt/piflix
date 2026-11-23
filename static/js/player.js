"use strict";
/* Reproductor: orquesta los componentes de js/player/ (resume, ui, transport, gestures, keys, next, platform).
   API pública: play(item, ctx?) y closePlayer(). `ctx` = { show, ep } cuando es un episodio. */
const player = $("player"), stage = $("stage"), video = $("video");
const BACKGROUND = ["top", "hero", "main", "foot"]; // se vuelven inertes mientras el reproductor está abierto
const SAVE_EVERY_MS = 5000;
const MEDIA_ERRORS = {
  2: "Se cortó la conexión con el servidor. Vuelve a intentarlo.",
  3: "El archivo está dañado o usa un códec que este navegador no decodifica.",
  4: "Este navegador no puede reproducir este formato (p. ej. MKV o H.265). Prueba con otro navegador.",
};
let lastFocus = null, curId = null, lastSave = 0;

/* ---------- componentes ---------- */
const ui = createPlayerUI({ stage, video, isBusy: () => transport.isSeeking() });
const transport = createTransport({ stage, video, ui });
const nextUp = createNextUp({
  play: (it, ctx) => play(it, ctx),
  onFinish: (show) => ui.toast(`Has terminado «${show.title}»`, 6000),
  leaveFullscreen: () => { if (document.fullscreenElement) document.exitFullscreen().catch(() => {}); },
});

/* ---------- acciones ---------- */
const toggle = () => (video.paused ? video.play().catch(() => {}) : video.pause());
const skip = (d) => {
  video.currentTime = Math.max(0, Math.min(video.duration || 0, video.currentTime + d));
  ui.flash(d < 0 ? "l" : "r");
};
async function toggleFs() {
  try {
    if (document.fullscreenElement) await document.exitFullscreen();
    else if (stage.requestFullscreen) {
      await stage.requestFullscreen();
      screen.orientation?.lock?.("landscape")?.catch(() => {}); // en el móvil, a horizontal
    } else if (stage.webkitRequestFullscreen) stage.webkitRequestFullscreen();
    else video.webkitEnterFullscreen?.(); // iPhone: reproductor nativo
  } catch {}
}
async function togglePip() {
  try {
    if (document.pictureInPictureElement) await document.exitPictureInPicture();
    else if (document.pictureInPictureEnabled) await video.requestPictureInPicture();
  } catch {}
}
const actions = {
  toggle, skip, fs: toggleFs, pip: togglePip, close: closePlayer, wake: ui.wake,
  mute: () => { video.muted = !video.muted; },
  volume: (d) => { video.volume = Math.max(0, Math.min(1, video.volume + d)); },
  seekPct: (p) => { if (video.duration) video.currentTime = video.duration * p; },
};
const clicks = { ...actions, speed: transport.cycleSpeed, back: () => skip(-10), fwd: () => skip(10) };
stage.addEventListener("click", (e) => {
  const b = e.target.closest("[data-act]");
  if (!b) return;
  clicks[b.dataset.act]?.();
  ui.wake();
});

const gestures = createGestures({ video, ui, toggle, skip, toggleFs });
const platform = createPlatform({ video, actions });
bindPlayerKeys({ isOpen: () => !player.hidden, a: actions });

/* ---------- reanudar y «visto» ---------- */
function persist(force) {
  const now = Date.now();
  if (!curId || !video.duration || (!force && now - lastSave < SAVE_EVERY_MS)) return;
  lastSave = now;
  Resume.save(curId, video.currentTime, video.duration);
}
video.addEventListener("loadedmetadata", () => {
  const p = Resume.get(curId);
  if (!p || !Resume.worth(p[0], video.duration)) return;
  video.currentTime = p[0];
  ui.toast(`Continúa en ${fmt(p[0])}` + (matchMedia("(pointer: fine)").matches ? " · tecla 0: desde el principio" : ""));
});
video.addEventListener("timeupdate", () => persist(false));
video.addEventListener("pause", () => persist(true));
video.addEventListener("ended", () => {
  if (curId) { Resume.clear(curId); Resume.markSeen(curId); }
  nextUp.offer();
});
addEventListener("pagehide", () => persist(true));
document.addEventListener("visibilitychange", () => { if (document.visibilityState === "hidden") persist(true); }); // móviles: no siempre hay pagehide

/* ---------- estado de carga y errores ---------- */
video.addEventListener("waiting", () => ui.setLoading(true));
for (const ev of ["playing", "canplay", "seeked"]) video.addEventListener(ev, () => ui.setLoading(false));
video.addEventListener("error", () => {
  ui.setLoading(false);
  const code = video.error?.code;
  if (!video.getAttribute("src") || code === 1) return; // se vació la fuente al cerrar / carga abortada a propósito
  ui.toast(MEDIA_ERRORS[code] || MEDIA_ERRORS[4], 0);
});

/* ---------- ciclo de vida ---------- */
function play(it, ctx = null) {
  persist(true); // si se encadena otro episodio, guarda el anterior
  nextUp.hide();
  nextUp.set(ctx);
  if (player.hidden || !lastFocus) lastFocus = document.activeElement;
  const title = it.title + (it.year ? ` (${it.year})` : "");
  curId = it.id;
  lastSave = 0;
  $("ptitle").textContent = title;
  ui.reset();
  video.setAttribute("aria-label", title);
  player.hidden = false;
  for (const id of BACKGROUND) $(id).inert = true;
  document.body.style.overflow = "hidden";
  video.src = `/api/media/${it.id}/stream`;
  video.playbackRate = 1;
  ui.setLoading(true);
  transport.reset();
  platform.open(title, ctx && nextUp.hasNext() ? nextUp.go : null);
  video.play().catch(() => {}); // autoplay: si el navegador lo bloquea queda en pausa
  stage.focus({ preventScroll: true }); // el foco entra al diálogo sin activar ningún botón
  ui.wake();
}
function closePlayer() {
  persist(true);
  curId = null; // los eventos que siguen ya no guardan nada
  nextUp.hide();
  nextUp.set(null);
  gestures.cancel();
  platform.close();
  video.pause();
  video.removeAttribute("src");
  video.load();
  ui.reset();
  player.hidden = true;
  for (const id of BACKGROUND) $(id).inert = false;
  document.body.style.overflow = "";
  if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
  lastFocus?.focus?.({ preventScroll: true });
  lastFocus = null;
}

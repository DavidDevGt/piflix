"use strict";
/* Transporte: barra de progreso (con búfer), volumen, velocidad y estados play/pausa/mute/pantalla completa
   reflejados en clases y etiquetas accesibles. Recuerda el volumen entre sesiones. */
function createTransport({ stage, video, ui }) {
  const seek = $("seek"), vol = $("vol"), timeEl = $("time");
  const SPEEDS = [1, 1.25, 1.5, 2, 0.75], VOL_KEY = "piflix-vol";
  const acts = (a) => stage.querySelectorAll(`[data-act="${a}"]`);
  let seeking = false;

  function syncTime() {
    const d = video.duration, t = seeking ? (seek.value / 1000) * (d || 0) : video.currentTime;
    if (!seeking && d) seek.value = (t / d) * 1000;
    seek.style.setProperty("--p", `${d ? (t / d) * 100 : 0}%`);
    const now = fmt(t);
    timeEl.textContent = `${now} / ${fmt(d)}`;
    seek.setAttribute("aria-valuetext", `${now} de ${fmt(d)}`);
  }
  function syncBuffer() {
    const d = video.duration;
    let end = 0;
    for (let i = 0; i < video.buffered.length; i++) {
      if (video.buffered.start(i) <= video.currentTime + 0.5) end = Math.max(end, video.buffered.end(i));
    }
    seek.style.setProperty("--b", `${d ? Math.min(100, (end / d) * 100) : 0}%`);
  }

  video.addEventListener("timeupdate", syncTime);
  for (const ev of ["progress", "loadedmetadata", "durationchange", "seeked"]) video.addEventListener(ev, () => { syncBuffer(); syncTime(); });

  const onPlayState = () => {
    stage.classList.toggle("paused", video.paused);
    for (const b of acts("toggle")) b.setAttribute("aria-label", video.paused ? "Reproducir" : "Pausar");
    ui.wake();
  };
  video.addEventListener("play", onPlayState);
  video.addEventListener("pause", onPlayState);

  const onVolume = () => {
    const off = video.muted || video.volume === 0;
    stage.classList.toggle("muted", off);
    for (const b of acts("mute")) b.setAttribute("aria-label", off ? "Activar sonido" : "Silenciar");
    vol.value = video.muted ? 0 : video.volume;
    vol.style.setProperty("--p", `${+vol.value * 100}%`);
    try { localStorage.setItem(VOL_KEY, JSON.stringify([video.volume, video.muted])); } catch {}
  };
  video.addEventListener("volumechange", onVolume);
  try {
    const [v, m] = JSON.parse(localStorage.getItem(VOL_KEY)) || [];
    if (typeof v === "number") video.volume = Math.min(1, Math.max(0, v));
    video.muted = !!m;
  } catch {}
  onVolume();

  video.addEventListener("ratechange", () => {
    const r = `${String(video.playbackRate).replace(".", ",")}×`;
    for (const b of acts("speed")) { b.textContent = r; b.setAttribute("aria-label", `Velocidad ${r}. Cambiar`); }
  });
  document.addEventListener("fullscreenchange", () => {
    const on = !!document.fullscreenElement;
    stage.classList.toggle("isfs", on);
    for (const b of acts("fs")) b.setAttribute("aria-label", on ? "Salir de pantalla completa" : "Pantalla completa");
    if (!on) screen.orientation?.unlock?.();
  });

  /* Sliders: el progreso solo salta al soltar (evita pedirle un rango al disco por cada píxel) */
  seek.addEventListener("input", () => { seeking = true; syncTime(); ui.wake(); });
  seek.addEventListener("change", () => {
    if (video.duration) video.currentTime = (seek.value / 1000) * video.duration;
    seeking = false;
  });
  seek.addEventListener("blur", () => { seeking = false; });
  vol.addEventListener("input", () => { video.volume = +vol.value; video.muted = +vol.value === 0; });

  return {
    isSeeking: () => seeking,
    cycleSpeed() { video.playbackRate = SPEEDS[(SPEEDS.indexOf(video.playbackRate) + 1) % SPEEDS.length]; },
    reset() { seek.value = 0; seeking = false; syncTime(); },
  };
}

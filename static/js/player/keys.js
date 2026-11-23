"use strict";
/* Atajos de teclado del reproductor. `a` = acciones que expone el reproductor:
   toggle, skip(±s), volume(±0..1), mute, fs, pip, close, seekPct(0..1) */
function bindPlayerKeys({ isOpen, a }) {
  document.addEventListener("keydown", (e) => {
    if (!isOpen() || e.ctrlKey || e.metaKey || e.altKey) return;
    const k = e.key.toLowerCase();
    const map = {
      " ": a.toggle, k: a.toggle,
      arrowleft: () => a.skip(-10), j: () => a.skip(-10),
      arrowright: () => a.skip(10), l: () => a.skip(10),
      arrowup: () => a.volume(0.05), arrowdown: () => a.volume(-0.05),
      m: a.mute, f: a.fs, p: a.pip,
      escape: () => { if (!document.fullscreenElement) a.close(); },
    };
    if (/^[0-9]$/.test(k)) map[k] = () => a.seekPct(+k / 10); // 0 = inicio, 1-9 = 10-90 %
    // Los botones y sliders enfocados conservan su comportamiento nativo con espacio/flechas
    const native = e.target.matches?.("button, input[type=range]") && (k === " " || k.startsWith("arrow"));
    if (map[k] && !native) {
      e.preventDefault();
      map[k]();
      a.wake?.();
    }
  });
}

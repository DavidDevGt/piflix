"use strict";
/* Gestos sobre el video: ratón (clic = pausa, doble clic = pantalla completa) y táctil
   (1 toque muestra/oculta controles; doble toque izquierda/derecha salta 10 s; centro, pausa). */
function createGestures({ video, ui, toggle, skip, toggleFs }) {
  let lastTouch = 0, lastTap = 0, tapTimer = 0, tapZone = "";

  video.addEventListener("pointerup", (e) => {
    if (e.pointerType === "mouse") return;
    const now = (lastTouch = Date.now());
    const r = video.getBoundingClientRect(), x = (e.clientX - r.left) / r.width;
    const zone = x < 0.33 ? "l" : x > 0.67 ? "r" : "c";
    if (now - lastTap < 320 && zone === tapZone) {
      clearTimeout(tapTimer);
      lastTap = 0;
      if (zone === "l") skip(-10);
      else if (zone === "r") skip(10);
      else toggle();
      ui.wake();
    } else {
      lastTap = now;
      tapZone = zone;
      tapTimer = setTimeout(ui.toggleChrome, 320);
    }
  });
  video.addEventListener("click", () => { if (Date.now() - lastTouch > 500) toggle(); });
  video.addEventListener("dblclick", () => { if (Date.now() - lastTouch > 500) toggleFs(); });

  return { cancel() { clearTimeout(tapTimer); } };
}

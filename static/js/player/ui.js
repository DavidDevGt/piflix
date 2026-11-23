"use strict";
/* Capa visual del reproductor: avisos, destello de salto, indicador de carga y auto-ocultar controles. */
function createPlayerUI({ stage, video, isBusy = () => false }) {
  const toastEl = $("toast"), flashEl = $("flash"), spinner = $("spinner");
  let idleTimer = 0, toastTimer = 0, flashTimer = 0;

  const ui = {
    toast(msg, ms = 4000) { // ms = 0: se queda hasta el siguiente aviso
      toastEl.textContent = msg;
      toastEl.classList.add("show");
      clearTimeout(toastTimer);
      if (ms) toastTimer = setTimeout(() => toastEl.classList.remove("show"), ms);
    },
    flash(side) {
      flashEl.className = `flash ${side}`;
      flashEl.textContent = side === "l" ? "−10 s" : "+10 s";
      void flashEl.offsetWidth; // reinicia la animación si se repite
      flashEl.classList.add("show");
      clearTimeout(flashTimer);
      flashTimer = setTimeout(() => flashEl.classList.remove("show"), 600);
    },
    setLoading(on) {
      spinner.hidden = !on;
      stage.classList.toggle("loading", on);
    },
    /* muestra los controles y los oculta a los 2,8 s si se está reproduciendo */
    wake() {
      stage.classList.remove("idle");
      clearTimeout(idleTimer);
      if (!video.paused) idleTimer = setTimeout(() => { if (!isBusy()) stage.classList.add("idle"); }, 2800);
    },
    toggleChrome() {
      if (stage.classList.contains("idle")) ui.wake();
      else { stage.classList.add("idle"); clearTimeout(idleTimer); }
    },
    reset() {
      clearTimeout(idleTimer);
      clearTimeout(toastTimer);
      toastEl.classList.remove("show");
      ui.setLoading(false);
    },
  };
  return ui;
}

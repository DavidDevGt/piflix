"use strict";
/* Siguiente episodio: al terminar uno, cuenta atrás de 8 s con «Reproducir ahora» / «Cancelar».
   ctx = { show, ep } de la serie en curso (null para películas). */
function createNextUp({ play, onFinish, leaveFullscreen }) {
  const box = $("next"), title = $("next-t"), count = $("next-n");
  let ctx = null, timer = 0;

  const following = () => {
    if (!ctx) return null;
    const list = ctx.show.episodes || [], i = list.findIndex((e) => e.id === ctx.ep.id);
    return i >= 0 ? list[i + 1] || null : null;
  };
  function hide() {
    clearInterval(timer);
    box.hidden = true;
  }
  function go() {
    const c = ctx, ep = following();
    hide();
    if (c && ep) play({ id: ep.id, title: epTitle(c.show, ep), year: null }, { show: c.show, ep });
  }
  function offer() {
    if (!ctx) return;
    const ep = following();
    if (!ep) { onFinish(ctx.show); return; }
    leaveFullscreen(); // la cuenta atrás y sus botones deben verse y poder pulsarse
    title.textContent = epTitle(ctx.show, ep);
    let n = 8;
    count.textContent = n;
    box.hidden = false;
    $("next-go").focus({ preventScroll: true });
    clearInterval(timer);
    timer = setInterval(() => {
      count.textContent = --n;
      if (n <= 0) go();
    }, 1000);
  }
  $("next-go").addEventListener("click", go);
  $("next-cancel").addEventListener("click", hide);

  return {
    set: (c) => { ctx = c; },
    hasNext: () => !!following(),
    go, hide, offer,
  };
}

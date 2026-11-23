"use strict";
/* ---------------- controles de la cabecera ---------------- */
function clearFilters() {
  qEl.value = "";
  yearEl.value = "";
  pendingYear = "";
  setKind("");
}
function setKind(k) {
  kind = k;
  markNav();
  load();
}
// Enlaces reales (<a href="?kind=…">): Ctrl/⌘-clic y clic central abren pestaña nueva
nav.addEventListener("click", (e) => {
  const a = e.target.closest("a");
  if (!a || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
  e.preventDefault();
  setKind(a.dataset.kind);
});

let tm;
// el buscador vive en un <form role="search">: Enter busca al instante y no recarga la página
$("search").addEventListener("submit", (e) => { e.preventDefault(); clearTimeout(tm); load(); qEl.blur(); });
// enlaces del pie: mismas categorías que la cabecera
$("foot").addEventListener("click", (e) => {
  const a = e.target.closest("a[data-kind]");
  if (!a || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
  e.preventDefault();
  setKind(a.dataset.kind);
  scrollTo({ top: 0 });
});
$("foot-add").addEventListener("click", () => $("dl-open").click());
qEl.addEventListener("input", () => { clearTimeout(tm); tm = setTimeout(load, 250); });
yearEl.addEventListener("change", () => { pendingYear = ""; load(); });

async function doReload() {
  setStatus("Recargando…");
  try {
    const r = await fetch("/api/reload", { method: "POST" });
    if (!r.ok) throw new Error((await r.json().catch(() => ({}))).error || "El servidor no respondió");
    pendingYear = yearEl.value || pendingYear;
    yearsLoaded = false;
    await load();
  } catch (e) {
    setStatus("");
    showState("plug", "No se pudo recargar", e.message + ". Vuelve a intentarlo en unos segundos.", "Reintentar", doReload, true);
  }
}
$("reload").addEventListener("click", doReload);
$("random").addEventListener("click", async () => {
  try {
    const r = await fetch("/api/media/random");
    if (!r.ok) throw new Error();
    const it = await r.json();
    if (isShow(it)) {
      const ep = it.episodes[Math.floor(Math.random() * it.episodes.length)];
      play({ id: ep.id, title: epTitle(it, ep), year: null }, { show: it, ep });
    } else play(it);
  } catch { setStatus("Tu biblioteca está vacía o no disponible."); }
});

const header = $("top");
const onScroll = () => header.classList.toggle("scrolled", window.scrollY > 24);
window.addEventListener("scroll", onScroll, { passive: true });
onScroll();

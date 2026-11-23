"use strict";
/* ---------------- estado del servidor ---------------- */
function footStat(rows) {
  $("foot-stat").replaceChildren(...rows.map(([lvl, text]) => {
    const li = el("li");
    const d = el("span", "dot" + (lvl === "ok" ? "" : " " + lvl));
    d.setAttribute("aria-hidden", "true");
    li.append(d, document.createTextNode(text));
    return li;
  }));
}
async function health() {
  try {
    const h = await (await fetch("/api/health")).json();
    const rows = [
      ["ok", `${nf.format(h.items)} título${h.items === 1 ? "" : "s"} en la biblioteca`],
      h.storage ? ["ok", "Disco conectado"] : ["bad", "Disco desconectado"],
    ];
    if (h.temp_c != null) rows.push([h.temp_c < 65 ? "ok" : h.temp_c < 75 ? "warn" : "bad", `Raspberry Pi a ${nf.format(Math.round(h.temp_c))}${NBSP}°C`]);
    if (h.enriching) rows.push(["warn", "Buscando carátulas…"]);
    footStat(rows);
    if (h.enriching) setTimeout(() => { health(); load(); }, 15000);
  } catch { footStat([["bad", "Sin conexión con el servidor"]]); }
}
$("foot-year").textContent = new Date().getFullYear();

/* Atajos: "/" enfoca el buscador; Esc dentro del buscador lo vacía y sale */
document.addEventListener("keydown", (e) => {
  if (!player.hidden || e.ctrlKey || e.metaKey || e.altKey) return;
  const tag = e.target.tagName;
  if (e.key === "/" && tag !== "INPUT" && tag !== "TEXTAREA" && tag !== "SELECT" && !document.querySelector("dialog[open]")) {
    e.preventDefault();
    qEl.focus();
    qEl.select();
  } else if (e.key === "Escape" && e.target === qEl) {
    if (qEl.value) { qEl.value = ""; load(); } else qEl.blur();
  }
});

/* Al cerrar el reproductor se actualiza «Seguir viendo» */
new MutationObserver(() => { if (player.hidden && !qEl.value.trim() && !kind) load(); })
  .observe(player, { attributes: true, attributeFilter: ["hidden"] });

markNav();
load();
health();

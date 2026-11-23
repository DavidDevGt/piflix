"use strict";
const $ = (id) => document.getElementById(id);
/* Icono del sprite icons.svg. fill=true para los macizos (play). */
const SVG_NS = "http://www.w3.org/2000/svg";
function icon(name, cls = "sm", fill = false) {
  const s = document.createElementNS(SVG_NS, "svg");
  s.setAttribute("class", `ico ${fill ? "f" : "s"} ${cls}`);
  s.setAttribute("aria-hidden", "true");
  s.setAttribute("focusable", "false");
  const u = document.createElementNS(SVG_NS, "use");
  u.setAttribute("href", `icons.svg#s-${name}`);
  s.append(u);
  return s;
}
const el = (tag, cls, text) => {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text != null) n.textContent = text;
  return n;
};
const nf = new Intl.NumberFormat("es");
/* segundos -> m:ss / h:mm:ss */
const fmt = (s) => {
  if (!isFinite(s)) return "0:00";
  s = Math.floor(s);
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), x = String(s % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${x}` : `${m}:${x}`;
};
const NBSP = " ";

const lib = $("library"), statusEl = $("status"), qEl = $("q"), yearEl = $("year"), nav = $("nav");
const KIND_LABEL = { pelicula: "Películas", serie: "Series" };

/* ---------------- estado (reflejado en la URL: ?q=&kind=&year=) ---------------- */
const url0 = new URLSearchParams(location.search);
let kind = url0.get("kind") in KIND_LABEL ? url0.get("kind") : "";
let pendingYear = /^\d{4}$/.test(url0.get("year") || "") ? url0.get("year") : "";
qEl.value = (url0.get("q") || "").slice(0, 100);
let loadSeq = 0, yearsLoaded = false;

function syncUrl() {
  const p = new URLSearchParams();
  if (qEl.value.trim()) p.set("q", qEl.value.trim());
  if (kind) p.set("kind", kind);
  const y = yearEl.value || pendingYear;
  if (y) p.set("year", y);
  const qs = p.toString();
  history.replaceState(null, "", qs ? "?" + qs : location.pathname);
}
function markNav() {
  for (const a of nav.children) {
    const on = a.dataset.kind === kind;
    a.classList.toggle("on", on);
    if (on) a.setAttribute("aria-current", "page"); else a.removeAttribute("aria-current");
  }
}

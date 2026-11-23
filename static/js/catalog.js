"use strict";
/* ---------------- catálogo ---------------- */
async function load() {
  const seq = ++loadSeq;
  syncUrl();
  const p = new URLSearchParams();
  if (qEl.value.trim()) p.set("q", qEl.value.trim());
  if (kind) p.set("kind", kind);
  if (yearEl.value || pendingYear) p.set("year", yearEl.value || pendingYear);
  if (!lib.children.length) { lib.replaceChildren(skeleton()); setStatus("Cargando biblioteca…"); }
  try {
    const r = await fetch("/api/media?" + p);
    const d = await r.json().catch(() => ({}));
    if (seq !== loadSeq) return;
    if (!r.ok) throw Object.assign(new Error(d.error || r.statusText), { friendly: !!d.error });
    fillYears(d.years);
    const searching = !!(qEl.value.trim() || yearEl.value);
    render(d.items, searching);
    renderHero(searching || kind ? null : d.items);
    setStatus(d.total ? `${searching ? "Resultados" : "Tu biblioteca"} · ${nf.format(d.total)} título${d.total === 1 ? "" : "s"}` : "");
  } catch (e) {
    if (seq !== loadSeq) return;
    renderHero(null);
    setStatus("");
    showState("plug", e.friendly ? e.message : "No se pudo conectar con tu servidor",
      e.friendly ? "Revisa que el disco esté conectado y vuelve a intentarlo." : "Comprueba que la Raspberry Pi está encendida y en tu red.",
      "Reintentar", load, true);
  }
}
const setStatus = (t) => { statusEl.textContent = t; };

/* Esqueleto: la página tiene forma al instante (Doherty) en vez de un círculo girando */
function skeleton() {
  const s = el("section", "sec");
  s.setAttribute("aria-hidden", "true");
  const g = el("div", "grid");
  for (let i = 0; i < 12; i++) g.append(el("div", "card sk"));
  s.append(el("div", "sk-h"), g);
  return s;
}

function fillYears(years) {
  if (yearsLoaded || !years.length) return;
  yearsLoaded = true;
  const cur = yearEl.value || pendingYear;
  yearEl.replaceChildren(new Option("Año", ""), ...years.map((y) => new Option(y, y)));
  yearEl.value = cur; // si el año ya no existe, queda en "Año"
  pendingYear = "";
}

function showState(ico, title, text, btn, fn, isError = false) {
  const s = el("div", "state");
  if (isError) s.setAttribute("role", "alert");
  if (isError) s.classList.add("err");
  const i = el("div", "state-ico");
  i.append(icon(ico, "xl"));
  s.append(i, el("h3", null, title), el("p", null, text));
  if (btn) {
    const b = el("button", "cta", btn);
    b.type = "button";
    b.addEventListener("click", fn);
    s.append(b);
  }
  lib.replaceChildren(s);
}

function card(it, opt = {}) {
  const b = el("button", "card");
  b.type = "button";
  const seen = opt.resume == null && isSeen(it);
  b.setAttribute("aria-label", (opt.label || (isShow(it) ? "Ver episodios de " : "Reproducir ") + it.title) + (seen ? ", visto" : ""));
  if (it.has_poster) {
    const img = el("img");
    img.loading = "lazy";
    img.decoding = "async";
    img.alt = "";
    img.width = 342;
    img.height = 513;
    img.src = `/api/img/${it.id}/poster?v=${it.poster_v || 0}`;
    b.classList.add("has-img");
    b.append(img);
  } else {
    b.append(el("div", "ph", it.title));
  }
  const meta = el("div", "meta");
  meta.append(el("div", "t", it.title));
  const st = isShow(it) ? showStats(it) : null;
  const sub = opt.sub || [it.year, st && st.text].filter(Boolean).join(" · ");
  if (sub) meta.append(el("div", "y", sub));
  const pl = el("span", "play");
  pl.append(isShow(it) ? icon("list", "") : icon("play", "", true));
  b.append(meta, pl);
  if (st && !opt.sub) {
    const bd = el("span", "badge", st.seasons ? "T" + st.seasons : plural(st.eps, "ep.", "ep."));
    bd.setAttribute("aria-hidden", "true");
    b.append(bd);
  }
  if (opt.resume != null) {
    const r = el("span", "resume");
    r.setAttribute("aria-hidden", "true");
    r.append(icon("play", "", true), document.createTextNode(" " + fmt(opt.resume)));
    b.append(r);
  }
  if (seen) {
    const m = el("span", "seenmark");
    m.setAttribute("aria-hidden", "true");
    m.append(icon("check", "sm"));
    b.append(m);
  }
  if (opt.progress != null) {
    const bar = el("span", "bar");
    bar.setAttribute("aria-hidden", "true");
    const fill = el("i");
    fill.style.width = `${Math.round(Math.max(3, Math.min(100, opt.progress * 100)))}%`;
    bar.append(fill);
    b.append(bar);
  }
  b.addEventListener("click", () => (opt.open ? opt.open() : openItem(it)));
  const w = el("div", "cw");
  w.append(b);
  if (!opt.noInfo && !isShow(it)) {
    const i = el("button", "card-info");
    i.type = "button";
    i.setAttribute("aria-label", `Más información de ${it.title}`);
    i.title = "Más información";
    i.append(icon("info", "sm"));
    i.addEventListener("click", () => openMovie(it));
    w.append(i);
  }
  return w;
}

/* ---------------- series: episodios ---------------- */
const isShow = (it) => it.kind === "serie";
let seenSet = new Set(); // ids vistos (se refresca en cada render)
const isSeen = (it) => (isShow(it) ? !!it.episodes?.length && it.episodes.every((e) => seenSet.has(e.id)) : seenSet.has(it.id));
const epLabel = (ep) => (ep.season ? `T${ep.season} E${ep.episode}` : ep.episode ? `E${ep.episode}` : "Episodio");
const epTitle = (show, ep) => `${show.title} · ${epLabel(ep)}${ep.title ? " — " + ep.title : ""}`;
const openItem = (it) => (isShow(it) ? openEpisodes(it) : play(it));
const plural = (n, one, many) => `${nf.format(n)} ${n === 1 ? one : many}`;

/* temporadas en orden de aparición: [[temporada|0, episodios[]], …] */
function bySeason(show) {
  const m = new Map();
  for (const ep of show.episodes || []) {
    const k = ep.season || 0;
    if (!m.has(k)) m.set(k, []);
    m.get(k).push(ep);
  }
  return [...m];
}
function showStats(show) {
  const seasons = bySeason(show).filter(([k]) => k).length;
  const eps = show.episodes?.length || 0;
  return { seasons, eps, text: [seasons ? plural(seasons, "temporada", "temporadas") : null, plural(eps, "episodio", "episodios")].filter(Boolean).join(" · ") };
}

const epsDlg = $("eps");
let epsShow = null; // serie abierta en la ficha (para el botón Editar)
const playEp = (show, ep) => {
  epsDlg.close(); // devuelve el foco a la tarjeta antes de abrir el reproductor
  play({ id: ep.id, title: epTitle(show, ep), year: null }, { show, ep });
};

/* cabecera de la ficha (película o serie) */
function fillSheet(it, metaParts) {
  epsShow = it;
  const img = $("eps-img");
  const src = it.has_backdrop ? `/api/img/${it.id}/backdrop` : it.has_poster ? `/api/img/${it.id}/poster?v=${it.poster_v || 0}` : "";
  img.hidden = !src;
  if (src) img.src = src; else img.removeAttribute("src");
  $("eps-head").classList.toggle("no-img", !src);
  $("eps-title").textContent = it.title;
  $("eps-meta").textContent = metaParts.filter(Boolean).join(" · ");
  $("eps-text").textContent = it.overview || "";
}
/* barra de avance de la ficha: fraction 0..1 o null para ocultarla */
function setSheetProgress(fraction, text) {
  $("eps-prog").hidden = fraction == null;
  if (fraction == null) return;
  $("eps-prog").querySelector("i").style.width = `${Math.round(Math.max(2, Math.min(100, fraction * 100)))}%`;
  $("eps-prog-t").textContent = text;
}
function setSheetPlay(label, fill, onClick) {
  const b = $("eps-play");
  b.hidden = !onClick;
  if (!onClick) return;
  b.replaceChildren(icon("play", "sm", true), document.createTextNode(" " + label));
  b.onclick = onClick;
}

function openMovie(it) {
  fillSheet(it, ["Película", it.year]);
  const p = Resume.get(it.id), seen = Resume.seen().has(it.id);
  setSheetPlay(p ? `Continuar desde ${fmt(p[0])}` : seen ? "Volver a ver" : "Reproducir", true, () => { epsDlg.close(); play(it); });
  if (p && p[2]) setSheetProgress(p[0] / p[2], `Te quedan ${Math.max(1, Math.round((p[2] - p[0]) / 60))} min`);
  else setSheetProgress(seen ? 1 : null, "Vista");
  $("eps-seasons").hidden = true;
  $("eps-body").replaceChildren();
  epsDlg.showModal();
  epsDlg.scrollTop = 0;
  $("eps-play").focus();
}
const openDetail = (it) => (isShow(it) ? openEpisodes(it) : openMovie(it));

function openEpisodes(show) {
  const groups = bySeason(show);
  const pos = Resume.all(), seen = Resume.seen();
  const all = show.episodes || [];
  // «Continuar»: el episodio en curso visto más recientemente; si no, el siguiente al último visto; si no, el primero
  const last = all.filter((e) => pos[e.id]).sort((x, y) => pos[y.id][1] - pos[x.id][1])[0];
  let lastSeen = -1;
  all.forEach((e, i) => { if (seen.has(e.id)) lastSeen = i; });
  const after = lastSeen >= 0 ? all[lastSeen + 1] : null;
  const target = last || after || all[0];
  const seenCount = all.filter((e) => seen.has(e.id)).length;

  fillSheet(show, [show.year, showStats(show).text]);
  const prefix = last ? "Continuar" : after ? "Siguiente:" : lastSeen >= 0 ? "Volver a ver" : "Reproducir";
  setSheetPlay(`${prefix} ${target ? epLabel(target) : ""}`.trim(), true, target && (() => playEp(show, target)));
  setSheetProgress(all.length && seenCount ? seenCount / all.length : null, `${seenCount} de ${all.length} episodios vistos`);

  const chips = $("eps-seasons"), body = $("eps-body");
  const draw = (key) => {
    for (const c of chips.children) c.setAttribute("aria-pressed", String(Number(c.dataset.s) === key));
    const eps = groups.find(([k]) => k === key)[1];
    const h = el("h3", null, key ? `Temporada ${key}` : "Episodios");
    h.append(el("small", null, plural(eps.length, "episodio", "episodios")));
    const rows = eps.map((ep) => {
      const b = el("button", "ep" + (seen.has(ep.id) ? " seen" : ""));
      b.type = "button";
      const t = pos[ep.id] ? pos[ep.id][0] : null;
      b.setAttribute("aria-label", `Reproducir ${epLabel(ep)}${ep.title ? ", " + ep.title : ""}${t != null ? ", en curso en " + fmt(t) : ""}`);
      const txt = el("span", "ep-txt");
      txt.append(el("span", "ep-name", ep.title || epLabel(ep)));
      if (t != null) txt.append(el("small", null, "En curso · " + fmt(t)));
      else if (seen.has(ep.id)) txt.append(el("small", "seen-t", "Visto"));
      if (t != null && pos[ep.id][2]) {
        const bar = el("span", "bar");
        bar.setAttribute("aria-hidden", "true");
        const fill = el("i");
        fill.style.width = `${Math.round(Math.max(3, Math.min(100, (t / pos[ep.id][2]) * 100)))}%`;
        bar.append(fill);
        txt.append(bar);
      }
      const pl = el("span", "ep-play");
      pl.append(icon("play", "sm", true));
      const num = el("b", null, ep.episode ? String(ep.episode).padStart(2, "0") : "·");
      b.append(num, txt, pl);
      b.addEventListener("click", () => playEp(show, ep));
      return b;
    });
    body.replaceChildren(h, ...rows);
  };

  const multi = groups.length > 1;
  chips.hidden = !multi;
  chips.replaceChildren(...(multi ? groups.map(([k]) => {
    const c = el("button", "chip", k ? `T${k}` : "Extras");
    c.type = "button";
    c.dataset.s = k;
    c.setAttribute("aria-label", k ? `Temporada ${k}` : "Episodios sin temporada");
    c.addEventListener("click", () => draw(k));
    return c;
  }) : []));
  draw(target ? target.season || 0 : groups[0]?.[0] ?? 0);
  epsDlg.showModal();
  epsDlg.scrollTop = 0;
}
$("eps-close").addEventListener("click", () => epsDlg.close());
epsDlg.addEventListener("click", (e) => { if (e.target === epsDlg) epsDlg.close(); });

function section(title, items) {
  const s = el("section", "sec");
  const h = el("h2", "sec-h", title);
  h.append(el("small", null, nf.format(items.length)));
  const g = el("div", "grid");
  g.append(...items.map((it) => card(it)));
  s.append(h, g);
  return s;
}

/* Seguir viendo: posiciones guardadas por el reproductor (solo en este navegador) */
function continueSection(items) {
  const byId = new Map();
  for (const it of items) {
    if (isShow(it)) for (const ep of it.episodes || []) byId.set(String(ep.id), { it, ep });
    else byId.set(String(it.id), { it });
  }
  const rows = Object.entries(Resume.all())
    .filter(([id]) => byId.has(id))
    .sort((a, b) => b[1][1] - a[1][1])
    .slice(0, 12);
  if (!rows.length) return null;
  const cards = rows.map(([id, [t, , d]]) => {
    const { it, ep } = byId.get(id);
    const name = ep ? epTitle(it, ep) : it.title;
    return card(it, {
      resume: t,
      progress: d ? t / d : null,
      label: `Continuar ${name} desde ${fmt(t)}`,
      noInfo: true,
      sub: ep ? epLabel(ep) : `Continúa en ${fmt(t)}`,
      open: ep ? () => play({ id: ep.id, title: epTitle(it, ep), year: null }, { show: it, ep }) : () => play(it),
    });
  });
  const s = el("section", "sec");
  const h = el("h2", "sec-h", "Seguir viendo");
  const g = el("div", "grid shelf");
  g.append(...cards);
  s.append(h, g);
  return s;
}

function render(items, searching) {
  seenSet = Resume.seen();
  if (!items.length) {
    if (searching) {
      return showState("search", "Sin resultados", "Prueba con otro título o quita los filtros.", "Limpiar filtros", clearFilters);
    }
    return showState("film", "Aún no hay nada por aquí",
      "Copia tus películas en la carpeta Peliculas y tus series en Series del disco, y pulsa Recargar.",
      "Recargar", doReload);
  }
  const secs = [];
  if (searching) secs.push(section("Resultados", items));
  else if (kind) secs.push(section(KIND_LABEL[kind], items));
  else {
    for (const k of Object.keys(KIND_LABEL)) {
      const g = items.filter((i) => i.kind === k);
      if (g.length) secs.push(section(KIND_LABEL[k], g));
    }
  }
  if (!searching && !kind) {
    const cont = continueSection(items);
    if (cont) secs.unshift(cont);
  }
  lib.replaceChildren(...secs);
}

/* Hero: solo con un título real de la biblioteca que tenga imagen de fondo.
   Se elige por día, así no cambia en cada recarga. Sin candidato, no se muestra. */
let featured = null;
function renderHero(items) {
  const hero = $("hero");
  const cands = (items || []).filter((i) => i.has_backdrop);
  featured = cands.length ? cands[Math.floor(Date.now() / 864e5) % cands.length] : null;
  hero.hidden = !featured;
  document.body.classList.toggle("no-hero", !featured);
  if (!featured) return;
  const img = $("hero-img");
  const src = `/api/img/${featured.id}/backdrop`;
  if (img.getAttribute("src") !== src) img.src = src;
  const meta = $("hero-meta");
  meta.replaceChildren(el("b", null, featured.kind === "serie" ? "Serie" : "Película"));
  if (featured.year) meta.append(" · " + featured.year);
  $("hero-title").textContent = featured.title;
  $("hero-text").textContent = featured.overview || "";
}
$("hero-play").addEventListener("click", () => featured && openItem(featured));

$("hero-info").addEventListener("click", () => featured && openDetail(featured));

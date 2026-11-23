"use strict";
/* ---------------- descargas ---------------- */
const dlDlg = $("dl"), dlForm = $("dl-form"), dlMsg = $("dl-msg"), dlList = $("dl-list"), dlToken = $("dl-token");
const TOKEN_KEY = "piflix-admin-token";
let jobsTimer, seenDone = new Set();
try { dlToken.value = localStorage.getItem(TOKEN_KEY) || ""; } catch {}

const mb = new Intl.NumberFormat("es", { maximumFractionDigits: 1 });
const fmtSize = (n) => (n >= 1e9 ? `${mb.format(n / 1e9)}${NBSP}GB` : `${mb.format(n / 1e6)}${NBSP}MB`);
const authHeaders = () => ({ "x-admin-token": dlToken.value });

/* ---------- series ya registradas: añadirles episodios sin retipear el nombre ---------- */
let showsList = [];
const normName = (t) => t.normalize("NFD").replace(/[\u0300-\u036f]/g, "").toLowerCase().replace(/[^a-z0-9]+/g, " ").trim();
const showById = (id) => showsList.find((x) => x.id === id);
function nextEpisode(show) {
  let s = 0, e = 0;
  for (const x of show.episodes || []) if (x.season > s || (x.season === s && x.episode > e)) { s = x.season; e = x.episode; }
  return { season: s || 1, episode: e + 1 };
}
function fillShowSelects() {
  for (const sel of [$("dl-show"), $("up-show")]) {
    const cur = sel.value;
    sel.replaceChildren(
      new Option("Nueva serie (escribir el nombre)", ""),
      ...showsList.map((x) => new Option(`${x.title} · ${plural(x.episodes?.length || 0, "episodio", "episodios")}`, x.id)),
    );
    sel.value = showsList.some((x) => x.id === cur) ? cur : "";
  }
  syncDlKind();
  if (typeof syncUp === "function") syncUp();
}
async function refreshShows() {
  try {
    const d = await (await fetch("/api/media?kind=serie")).json();
    showsList = (d.items || []).filter((x) => x.kind === "serie");
  } catch { return; }
  fillShowSelects();
}

function syncDlKind() {
  const serie = $("dl-kind").value === "serie";
  const existing = serie && !!$("dl-show").value;
  $("dl-show-w").hidden = !serie || !showsList.length;
  $("dl-name-w").hidden = existing;
  $("dl-name").required = !existing;
  $("dl-ep").hidden = !serie;
  $("dl-year-l").hidden = serie;
  $("dl-name-l").textContent = serie ? "Nombre de la serie" : "Título de la película";
  $("dl-season").required = serie;
  $("dl-episode").required = serie;
}
$("dl-kind").addEventListener("change", syncDlKind);
$("dl-show").addEventListener("change", () => {
  const sh = showById($("dl-show").value);
  if (sh) { const n = nextEpisode(sh); $("dl-season").value = n.season; $("dl-episode").value = n.episode; }
  syncDlKind();
});
syncDlKind();

$("dl-open").addEventListener("click", () => { dlDlg.showModal(); pollJobs(); refreshShows(); });
$("dl-close").addEventListener("click", () => dlDlg.close());
dlDlg.addEventListener("close", () => clearTimeout(jobsTimer));
dlDlg.addEventListener("click", (e) => { if (e.target === dlDlg) dlDlg.close(); });

dlForm.addEventListener("submit", async (e) => {
  e.preventDefault();
  const btn = $("dl-go");
  const serie = $("dl-kind").value === "serie";
  const showId = serie ? $("dl-show").value : "";
  const body = {
    url: $("dl-url").value.trim(),
    kind: $("dl-kind").value,
    title: showId ? showById(showId)?.title || "" : $("dl-name").value.trim(),
    show: showId || null,
    year: serie ? null : Number($("dl-year").value) || null,
    season: serie ? Number($("dl-season").value) : null,
    episode: serie ? Number($("dl-episode").value) : null,
    referer: $("dl-referer").value.trim() || null,
  };
  btn.disabled = true;
  btn.textContent = "Comprobando enlace…";
  dlMsg.textContent = "";
  try {
    const r = await fetch("/api/downloads", {
      method: "POST",
      headers: { "content-type": "application/json", ...authHeaders() },
      body: JSON.stringify(body),
    });
    const d = await r.json().catch(() => ({}));
    if (!r.ok) throw new Error(d.error || `El servidor respondió ${r.status}`);
    try { localStorage.setItem(TOKEN_KEY, dlToken.value); } catch {}
    $("dl-url").value = "";
    $("dl-referer").value = "";
    $("dl-name").value = showId ? "" : "";
    refreshShows();
    $("dl-year").value = "";
    $("dl-episode").value = serie ? String(body.episode + 1) : "";
    pollJobs();
  } catch (err) {
    dlMsg.textContent = err.message;
  } finally {
    btn.disabled = false;
    btn.textContent = "Iniciar descarga";
  }
});

function renderJobs(jobs) {
  const STATE = {
    cola: "En cola…", descargando: "Descargando…", listo: "Listo", error: "Error",
    cancelado: "Cancelada (avance conservado)", enlace_caducado: "Enlace caducado", fallo: "Falló (avance conservado)",
  };
  const RESUMABLE = new Set(["cancelado", "error", "fallo", "enlace_caducado"]);
  const KEEPS_BYTES = new Set(["descargando", "listo", "cancelado", "fallo", "enlace_caducado"]);

  const act = async (j, path, method, body) => {
    dlMsg.textContent = "";
    try {
      const r = await fetch(`/api/downloads/${j.id}${path}`, {
        method,
        headers: { ...authHeaders(), ...(body ? { "content-type": "application/json" } : {}) },
        body: body ? JSON.stringify(body) : undefined,
      });
      if (!r.ok) dlMsg.textContent = (await r.json().catch(() => ({}))).error || "No se pudo completar la acción";
    } catch { dlMsg.textContent = "No se pudo conectar con el servidor"; }
    pollJobs();
  };
  const btn = (label, j, fn) => {
    const b = el("button", "btn", label);
    b.type = "button";
    b.setAttribute("aria-label", `${label} ${j.name}`);
    b.addEventListener("click", fn);
    return b;
  };

  const items = [...jobs].reverse().map((j) => {
    const active = j.state === "cola" || j.state === "descargando";
    const li = el("li", "job " + j.state);
    const top = el("div", "job-top");
    top.append(el("div", "job-name", j.name));
    const acts = el("div", "job-acts");
    let urlIn = null;
    if (active) {
      acts.append(btn("Cancelar", j, () => act(j, "", "DELETE")));
    } else if (RESUMABLE.has(j.state)) {
      if (j.state === "enlace_caducado") {
        urlIn = el("input");
        urlIn.type = "url";
        urlIn.placeholder = "Enlace nuevo (https://…)";
        urlIn.setAttribute("aria-label", `Enlace nuevo para ${j.name}`);
        urlIn.spellcheck = false;
      }
      acts.append(btn(j.state === "error" ? "Reintentar" : "Reanudar", j, () => {
        const u = urlIn && urlIn.value.trim();
        act(j, "/resume", "POST", u ? { url: u } : null);
      }));
      acts.append(btn("Descartar", j, () => act(j, "/discard", "POST")));
    } else {
      acts.append(btn("Quitar", j, () => act(j, "", "DELETE")));
    }
    top.append(acts);
    li.append(top);
    if (active) {
      const p = el("progress");
      p.max = j.total || 1;
      p.value = j.total ? j.bytes : 0;
      if (!j.total) p.removeAttribute("value"); // barra indeterminada
      p.setAttribute("aria-label", "Progreso de " + j.name);
      li.append(p);
    }
    const pct = j.total && j.bytes ? ` · ${Math.floor((j.bytes / j.total) * 100)}%` : "";
    const size = KEEPS_BYTES.has(j.state) && j.bytes ? ` · ${fmtSize(j.bytes)}${j.total ? " de " + fmtSize(j.total) : ""}` : "";
    const retry = j.state === "descargando" && j.intentos > 0 ? ` · reintento ${j.intentos}/3` : "";
    li.append(el("div", "job-meta", `${STATE[j.state] || j.state}${pct}${size}${retry} · ${j.host}${j.error ? " — " + j.error : ""}`));
    if (urlIn) li.append(urlIn);
    return li;
  });
  dlList.replaceChildren(...items);
}

async function pollJobs() {
  clearTimeout(jobsTimer);
  if (!dlDlg.open) return;
  let active = false;
  if (!dlToken.value) {
    dlList.replaceChildren();
  } else {
    try {
      const r = await fetch("/api/downloads", { headers: authHeaders() });
      const d = await r.json().catch(() => ({}));
      if (!r.ok) throw new Error(d.error || "");
      renderJobs(d.jobs);
      active = d.jobs.some((j) => j.state === "cola" || j.state === "descargando");
      let fresh = false;
      for (const j of d.jobs) if (j.state === "listo" && !seenDone.has(j.id)) { seenDone.add(j.id); fresh = true; }
      if (fresh) { yearsLoaded = false; load(); health(); } // aparece en el catálogo sin recargar
    } catch (err) {
      dlList.replaceChildren();
      if (err.message) dlMsg.textContent = err.message;
    }
  }
  jobsTimer = setTimeout(pollJobs, active ? 1500 : 6000);
}

"use strict";
/* ---------------- subir archivos desde el navegador (uno, varios o una carpeta) ---------------- */
const upForm = $("up-form"), upMsg = $("up-msg"), upBar = $("up-bar"), upGo = $("up-go"), upFile = $("up-file"), upDir = $("up-dir"),
  upToken = $("up-token"), upRows = $("up-rows"), upInfo = $("up-info");
try { upToken.value = localStorage.getItem(TOKEN_KEY) || ""; } catch {}
let upXhr = null, upBusy = false, upCancel = false, upSeq = 0;
let upPicked = [];   // archivos elegidos (solo videos)
let batch = null;    // modo álbum: { items: [{ f, a, rel, season, episode, guess, done, … }] }
const VIDEO_RE = /\.(mp4|m4v|mkv|webm|mov|avi)$/i;

function setMode(up) {
  dlForm.hidden = up;
  upForm.hidden = !up;
  $("tab-link").setAttribute("aria-pressed", String(!up));
  $("tab-up").setAttribute("aria-pressed", String(up));
}
$("tab-link").addEventListener("click", () => setMode(false));
$("tab-up").addEventListener("click", () => setMode(true));
dlDlg.addEventListener("close", () => { upCancel = true; if (upXhr) upXhr.abort(); });

const upKind = () => $("up-kind").value;

/* Muestra u oculta campos según el modo (un archivo / álbum) y la categoría; `required` solo en los visibles */
function syncUp() {
  const serie = upKind() === "serie", multi = !!batch;
  const existing = serie && !!$("up-show").value;
  $("up-show-w").hidden = !serie || !showsList.length;
  const showName = serie ? !existing : !multi;
  $("up-name-w").hidden = !showName;
  $("up-name").required = showName;
  $("up-name-l").textContent = serie ? "Nombre de la serie" : "Título de la película";
  $("up-year-l").hidden = serie || multi;
  $("up-ep").hidden = !serie || multi;
  $("up-season").required = $("up-episode").required = serie && !multi;
  $("up-batch").hidden = !multi;
  $("up-all-l").hidden = !serie;
  if (!upBusy) upGo.textContent = multi ? `Subir ${batch.items.length} archivos` : "Subir a la biblioteca";
}
$("up-kind").addEventListener("change", () => { if (batch) renderRows(); syncUp(); });
$("up-show").addEventListener("change", () => {
  const sh = showById($("up-show").value);
  if (sh && !batch) { const n = nextEpisode(sh); $("up-season").value = n.season; $("up-episode").value = n.episode; }
  syncUp();
});
/* si el nombre detectado coincide con una serie ya registrada, se elige sola */
function autoShow(name) {
  const k = normName(name || "");
  const hit = k && showsList.find((x) => normName(x.title) === k);
  $("up-show").value = hit ? hit.id : "";
  syncUp();
  return hit;
}
// el diálogo ya cargó la lista al abrirse; al entrar en esta pestaña se refresca por si cambió
$("tab-up").addEventListener("click", () => refreshShows());
syncUp();

/* ---------- metadatos del archivo ----------
   MP4/M4V/MOV: átomos iTunes (©nam título, ©day año, tvsh serie, tvsn temporada, tves episodio) y duración.
   MKV/WebM: Title y Duration de Info. Cualquier formato: resolución y duración vía <video>.
   Todo se lee en el navegador; solo se leen unos KB del archivo, nunca se sube nada. */
const u32 = (v, o) => v.getUint32(o);
const tag4 = (v, o) => String.fromCharCode(v.getUint8(o), v.getUint8(o + 1), v.getUint8(o + 2), v.getUint8(o + 3));
const utf8 = new TextDecoder("utf-8");

async function mp4Meta(file) {
  const out = {};
  // cajas de primer nivel: se salta de una a otra hasta encontrar moov (suele estar al inicio o al final)
  let pos = 0, moov = null;
  for (let i = 0; i < 64 && pos + 8 <= file.size; i++) {
    const v = new DataView(await file.slice(pos, pos + 16).arrayBuffer());
    let size = u32(v, 0), hdr = 8;
    if (size === 1 && v.byteLength >= 16) { size = Number(v.getBigUint64(8)); hdr = 16; }
    else if (size === 0) size = file.size - pos;
    if (size < hdr) break;
    if (tag4(v, 4) === "moov") { moov = { pos: pos + hdr, end: pos + size }; break; }
    pos += size;
  }
  if (!moov || moov.end - moov.pos > 32 * 1024 * 1024) return out;
  const buf = await file.slice(moov.pos, moov.end).arrayBuffer();
  const v = new DataView(buf);
  const kids = (from, to) => {
    const r = [];
    for (let o = from; o + 8 <= to;) {
      let size = u32(v, o), hdr = 8;
      if (size === 1) { size = Number(v.getBigUint64(o + 8)); hdr = 16; }
      if (size < hdr || o + size > to) break;
      r.push({ type: tag4(v, o + 4), start: o + hdr, end: o + size });
      o += size;
    }
    return r;
  };
  const find = (list, t) => list.find((k) => k.type === t);
  const top = kids(0, buf.byteLength);
  const mvhd = find(top, "mvhd");
  if (mvhd) {
    const ver = v.getUint8(mvhd.start);
    const ts = u32(v, mvhd.start + (ver === 1 ? 20 : 12));
    const dur = ver === 1 ? Number(v.getBigUint64(mvhd.start + 24)) : u32(v, mvhd.start + 16);
    if (ts) out.duration = dur / ts;
  }
  const udta = find(top, "udta");
  const meta = (udta && find(kids(udta.start, udta.end), "meta")) || find(top, "meta");
  const ilst = meta && find(kids(meta.start + 4, meta.end), "ilst"); // meta es «full box»: 4 bytes de versión
  if (!ilst) return out;
  for (const it of kids(ilst.start, ilst.end)) {
    const d = find(kids(it.start, it.end), "data");
    if (!d || d.end - d.start < 8) continue;
    const kind = v.getUint32(d.start) & 0xffffff, bytes = new Uint8Array(buf, d.start + 8, d.end - d.start - 8);
    const num = () => bytes.reduce((a, b) => a * 256 + b, 0);
    const t = it.type;
    if (t === "\xa9nam") out.title = utf8.decode(bytes);
    else if (t === "\xa9day") out.year = (utf8.decode(bytes).match(/\d{4}/) || [])[0];
    else if (t === "tvsh") out.show = utf8.decode(bytes);
    else if (t === "tvsn" && kind !== 1) out.season = num();
    else if (t === "tves" && kind !== 1) out.episode = num();
    else if (t === "stik") out.stik = num();
  }
  return out;
}

async function mkvMeta(file) {
  const out = {};
  const buf = await file.slice(0, 512 * 1024).arrayBuffer();
  const v = new DataView(buf), n = buf.byteLength;
  const vint = (o, keepMarker) => { // devuelve [valor, longitud] (id conserva el marcador; tamaño no)
    if (o >= n) return null;
    const b = v.getUint8(o);
    let len = 1;
    while (len <= 8 && !(b & (0x80 >> (len - 1)))) len++;
    if (len > 8 || o + len > n) return null;
    let val = keepMarker ? b : b & (0xff >> len);
    for (let i = 1; i < len; i++) val = val * 256 + v.getUint8(o + i);
    return [val, len];
  };
  const walk = (from, to, depth) => {
    let scale = 1e6, dur = null;
    for (let o = from; o < to;) {
      const id = vint(o, true); if (!id) break;
      const sz = vint(o + id[1], false); if (!sz) break;
      const body = o + id[1] + sz[1];
      const unknown = sz[0] >= 2 ** 48; // tamaño desconocido (streaming)
      const end = unknown ? to : Math.min(body + sz[0], to);
      if (id[0] === 0x18538067 && depth === 0) walk(body, end, 1); // Segment
      else if (id[0] === 0x1549a966 && depth === 1) { // Info
        const r = walk(body, end, 2);
        if (r && r.dur != null) out.duration = (r.dur * r.scale) / 1e9;
      } else if (depth === 2) {
        if (id[0] === 0x7ba9) out.title = utf8.decode(new Uint8Array(buf, body, end - body)).replace(/\0+$/, "");
        else if (id[0] === 0x2ad7b1) scale = new Uint8Array(buf, body, end - body).reduce((a, b) => a * 256 + b, 0);
        else if (id[0] === 0x4489) dur = end - body === 8 ? v.getFloat64(body) : end - body === 4 ? v.getFloat32(body) : null;
      }
      if (unknown && depth < 2) return;
      o = end;
    }
    return depth === 2 ? { scale, dur } : undefined;
  };
  walk(0, n, 0);
  return out;
}

/* resolución y duración con el propio navegador (si sabe decodificar el formato) */
function videoProps(file) {
  return new Promise((res) => {
    const u = URL.createObjectURL(file), vd = document.createElement("video");
    const done = (r) => { URL.revokeObjectURL(u); vd.removeAttribute("src"); res(r); };
    vd.preload = "metadata";
    vd.onloadedmetadata = () => done({ w: vd.videoWidth, h: vd.videoHeight, duration: isFinite(vd.duration) ? vd.duration : null });
    vd.onerror = () => done({});
    setTimeout(() => done({}), 4000);
    vd.src = u;
  });
}

/* título, temporada y episodio a partir del nombre: «Serie.S02E05.1080p.mkv», «Serie 2x05», «Capitulo 05» */
function fromName(name) {
  const out = {};
  let n = name.replace(/\.[^.]+$/, "").replace(/[._]+/g, " ");
  let m = n.match(/\bs(\d{1,2})\s*e(\d{1,3})\b/i) || n.match(/\b(\d{1,2})x(\d{2,3})\b/i);
  if (m) {
    out.season = Number(m[1]);
    out.episode = Number(m[2]);
    n = n.slice(0, m.index);
  } else if ((m = n.match(/\b(?:cap(?:[ií]tulo)?|ep(?:isodio)?)\s*\.?\s*(\d{1,3})\b/i) || n.match(/\be(\d{1,3})\b/i) || n.match(/\s-\s*(\d{1,3})\s*$/))) {
    out.episode = Number(m[1]);
    n = n.slice(0, m.index);
  } else {
    const y = n.match(/\b(19|20)\d{2}\b/);
    if (y) { out.year = y[0]; n = n.slice(0, y.index); }
  }
  n = n.replace(/[\[\(].*$/, "").replace(/[\s-]+$/, "").replace(/\s+/g, " ").trim();
  if (n) out.title = n.slice(0, 80);
  return out;
}

/* Reúne todo lo que se sabe de un archivo: etiquetas internas > nombre > carpetas (Serie/Temporada 2/…) */
async function analyze(f, probe) {
  const ext = (f.name.split(".").pop() || "").toLowerCase();
  const tagged = /^(mp4|m4v|mov)$/.test(ext) ? mp4Meta(f) : /^(mkv|webm)$/.test(ext) ? mkvMeta(f) : Promise.resolve({});
  const [tags, vp] = await Promise.all([tagged.catch(() => ({})), probe ? videoProps(f) : Promise.resolve({})]);
  const nm = fromName(f.name);
  let folderSeason = null, folderShow = "";
  for (const part of (f.webkitRelativePath || "").split("/").slice(0, -1)) {
    const m = part.match(/^(?:temporada|season|t|s)[\s._-]*0*(\d{1,2})$/i);
    if (m) folderSeason = Number(m[1]);
    else if (!folderShow) folderShow = part.slice(0, 80);
  }
  const embeddedShow = !!(tags.show || tags.stik === 10 || tags.season != null || tags.episode != null);
  const serie = embeddedShow || nm.season != null || nm.episode != null || folderSeason != null;
  return {
    tags, vp, serie, folderShow,
    title: (embeddedShow ? tags.show || tags.title : tags.title) || nm.title || "",
    show: tags.show || "",
    season: tags.season ?? nm.season ?? folderSeason ?? null,
    episode: tags.episode ?? nm.episode ?? null,
    year: tags.year || nm.year || "",
    fromTags: !!(tags.title || tags.show || tags.year),
  };
}

/* un solo archivo: rellena el formulario normal */
function applySingle(f, a) {
  $("up-kind").value = a.serie ? "serie" : "pelicula";
  $("up-name").value = (a.title || a.folderShow).slice(0, 80);
  $("up-year").value = a.serie ? "" : a.year;
  $("up-season").value = a.serie && a.season != null ? String(a.season) : "";
  $("up-episode").value = a.serie && a.episode != null ? String(a.episode) : "";
  const known = a.serie && autoShow($("up-name").value);
  if (known && a.episode == null) { const n = nextEpisode(known); $("up-season").value = n.season; $("up-episode").value = n.episode; }
  syncUp();
  const dur = a.tags.duration ?? a.vp.duration;
  const bits = [
    a.serie ? `Serie${a.season != null ? ` · T${a.season}` : ""}${a.episode != null ? ` E${a.episode}` : ""}` : null,
    a.vp.w ? `${a.vp.w}×${a.vp.h}` : null,
    dur ? fmt(dur) : null,
    fmtSize(f.size),
  ].filter(Boolean);
  upInfo.textContent = `Detectado (${a.fromTags ? "etiquetas del archivo" : "nombre del archivo"}): ${bits.join(" · ")}. Revisa los campos antes de subir.`;
}

const mostCommon = (arr) => {
  const c = new Map();
  for (const x of arr) c.set(x, (c.get(x) || 0) + 1);
  return [...c].sort((x, y) => y[1] - x[1])[0]?.[0] || "";
};

/* álbum: varios archivos → una fila editable por archivo */
function applyBatch(items) {
  const serie = items.filter((i) => i.a.serie).length * 2 >= items.length;
  $("up-kind").value = serie ? "serie" : "pelicula";
  $("up-name").value = serie ? mostCommon(items.map((i) => i.a.show || i.a.title || i.a.folderShow).filter(Boolean)) : "";
  $("up-all").value = "";
  items.forEach((it, i) => {
    it.season = it.a.season ?? 1;
    it.episode = it.a.episode ?? i + 1;
    it.guess = it.a.episode == null; // numeración supuesta por el orden: se marca para revisarla
    it.done = false;
  });
  batch = { items };
  renderRows();
  if (serie) autoShow($("up-name").value); else $("up-show").value = "";
  syncUp();
  const guessed = items.filter((i) => i.guess).length;
  upInfo.textContent = `${items.length} videos (${fmtSize(items.reduce((s, i) => s + i.f.size, 0))}). ` +
    (guessed && serie ? `${guessed} sin número de episodio detectado: se numeraron por orden, revísalos. ` : "") +
    "Todos los campos son opcionales de ajustar; lo detectado se usa tal cual.";
}

function field(label, inp, cls) {
  const w = el("label", "urow-n " + (cls || ""));
  w.append(el("span", null, label), inp);
  return w;
}
function numInput(value, min, max, aria, guess) {
  const i = el("input");
  i.type = "number";
  i.inputMode = "numeric";
  i.min = min;
  i.max = max;
  i.value = value == null ? "" : String(value);
  i.setAttribute("aria-label", aria);
  if (guess) i.classList.add("guess");
  return i;
}
function renderRows() {
  const serie = upKind() === "serie";
  upRows.replaceChildren(...batch.items.map((it) => {
    const li = el("li", "urow");
    const nm = el("span", "urow-name", it.f.name);
    nm.title = it.f.webkitRelativePath || it.f.name;
    const f = el("div", "urow-f");
    if (serie) {
      it.sIn = numInput(it.season, 0, 99, `Temporada de ${it.f.name}`, false);
      it.eIn = numInput(it.episode, 1, 999, `Episodio de ${it.f.name}`, it.guess);
      f.append(field("T", it.sIn), field("E", it.eIn));
    } else {
      it.tIn = el("input");
      it.tIn.maxLength = 80;
      it.tIn.value = it.a.title || it.f.name.replace(/\.[^.]+$/, "");
      it.tIn.setAttribute("aria-label", `Título de ${it.f.name}`);
      it.yIn = numInput(it.a.year, 1900, 2100, `Año de ${it.f.name}`, false);
      f.append(field("Título", it.tIn, "grow"), field("Año", it.yIn));
    }
    it.st = el("span", "urow-st");
    const rm = el("button", "urow-x");
    rm.type = "button";
    rm.setAttribute("aria-label", `Quitar ${it.f.name} de la lista`);
    rm.append(icon("x", "sm"));
    rm.addEventListener("click", () => {
      if (upBusy) return;
      batch.items = batch.items.filter((x) => x !== it);
      if (!batch.items.length) { clearPick(); return; }
      renderRows();
      syncUp();
    });
    li.append(nm, f, it.st, rm);
    return li;
  }));
}
$("up-all").addEventListener("input", (e) => {
  if (e.target.value === "" || !batch) return;
  for (const it of batch.items) if (it.sIn) it.sIn.value = e.target.value;
});

function clearPick() {
  upPicked = [];
  batch = null;
  upFile.value = "";
  upDir.value = "";
  upInfo.textContent = "";
  syncUp();
}

async function onPick(list) {
  const seq = ++upSeq;
  upMsg.className = "msg";
  upMsg.textContent = "";
  upInfo.textContent = "";
  batch = null;
  const files = [...list].filter((f) => VIDEO_RE.test(f.name))
    .sort((x, y) => (x.webkitRelativePath || x.name).localeCompare(y.webkitRelativePath || y.name, undefined, { numeric: true }));
  upPicked = files;
  if (!files.length) {
    syncUp();
    if (list.length) upInfo.textContent = "No hay videos en esa selección (mp4, mkv, webm, avi, mov, m4v).";
    return;
  }
  if (files.length === 1) {
    const a = await analyze(files[0], true);
    if (seq === upSeq) applySingle(files[0], a);
    return;
  }
  const items = [];
  for (const [i, f] of files.entries()) {
    upInfo.textContent = `Leyendo datos de los archivos… ${i + 1}/${files.length}`;
    items.push({ f, a: await analyze(f, false) });
    if (seq !== upSeq) return; // eligió otra cosa mientras se leía
  }
  applyBatch(items);
}
upFile.addEventListener("change", () => onPick(upFile.files));
upDir.addEventListener("change", () => onPick(upDir.files));
$("up-dir-btn").addEventListener("click", () => upDir.click());

/* ---------- envío ---------- */
function sendFile(url, file, token, onProgress) {
  return new Promise((resolve, reject) => {
    const x = new XMLHttpRequest();
    upXhr = x;
    x.open("POST", url);
    x.setRequestHeader("x-admin-token", token);
    x.upload.onprogress = (ev) => ev.lengthComputable && onProgress(ev.loaded, ev.total);
    x.onload = () => {
      let d = {};
      try { d = JSON.parse(x.responseText); } catch {}
      if (x.status >= 200 && x.status < 300) resolve(d);
      else reject(Object.assign(new Error(d.error || `El servidor respondió ${x.status}`), { status: x.status }));
    };
    x.onerror = () => reject(new Error("Se perdió la conexión durante la subida (si entras por el túnel, prueba desde la red local)"));
    x.onabort = () => reject(Object.assign(new Error("Subida cancelada"), { aborted: true }));
    x.send(file);
  });
}

/* sondeo (clave, nombre, espacio y formato sin enviar el archivo) + envío */
async function uploadOne(file, p, token, onProgress) {
  const r = await fetch("/api/upload?" + p, { method: "POST", headers: { "x-admin-token": token, "x-upload-check": "1", "x-upload-size": String(file.size) } });
  if (!r.ok) throw Object.assign(new Error((await r.json().catch(() => ({}))).error || `El servidor respondió ${r.status}`), { status: r.status });
  try { localStorage.setItem(TOKEN_KEY, token); } catch {}
  return sendFile("/api/upload?" + p, file, token, onProgress);
}
const extOf = (f) => (f.name.split(".").pop() || "mp4").toLowerCase();

function busy(on) {
  upBusy = on;
  upBar.hidden = !on;
  if (on) { upBar.value = 0; upGo.textContent = "Cancelar subida"; } else syncUp();
}

async function submitSingle(token) {
  const f = upPicked[0], serie = upKind() === "serie";
  const showId = serie ? $("up-show").value : "";
  const p = new URLSearchParams({ kind: upKind(), title: showId ? showById(showId)?.title || "" : $("up-name").value.trim(), ext: extOf(f) });
  if (showId) p.set("show", showId);
  if (serie) { p.set("season", $("up-season").value); p.set("episode", $("up-episode").value); }
  else if ($("up-year").value) p.set("year", $("up-year").value);
  upGo.textContent = "Comprobando…";
  upGo.disabled = true;
  try {
    const d = await uploadOne(f, p, token, (a, b) => {
      if (!upBusy) { upGo.disabled = false; busy(true); }
      upBar.max = b;
      upBar.value = a;
      upMsg.textContent = `${Math.floor((a / b) * 100)}% · ${fmtSize(a)} de ${fmtSize(b)}`;
    });
    upMsg.className = "msg ok";
    upMsg.textContent = `Listo: «${d.name}» ya está en la biblioteca`;
    const next = serie ? String(Number($("up-episode").value) + 1) : "";
    clearPick();
    $("up-name").value = serie && !showId ? p.get("title") : "";
    $("up-episode").value = next;
    $("up-season").value = serie ? p.get("season") : "";
    load();
    refreshShows();
  } catch (err) {
    upMsg.textContent = err.message;
  } finally {
    upGo.disabled = false;
    busy(false);
  }
}

async function submitBatch(token) {
  const serie = upKind() === "serie", showId = serie ? $("up-show").value : "";
  const name = showId ? showById(showId)?.title || "" : $("up-name").value.trim();
  if (serie && !name) { upMsg.textContent = "Escribe el nombre de la serie"; return; }
  const jobs = [], seen = new Set();
  for (const it of batch.items) {
    const p = new URLSearchParams({ kind: upKind(), ext: extOf(it.f) });
    if (serie) {
      const s = it.sIn.value, e = it.eIn.value, key = `${s}x${e}`;
      if (s === "" || e === "") { upMsg.textContent = `Falta temporada o episodio en «${it.f.name}»`; return; }
      if (seen.has(key)) { upMsg.textContent = `Dos archivos tienen T${s} E${e}: corrige la numeración`; return; }
      seen.add(key);
      p.set("title", name);
      if (showId) p.set("show", showId);
      p.set("season", s);
      p.set("episode", e);
    } else {
      const t = it.tIn.value.trim();
      if (!t) { upMsg.textContent = `Falta el título de «${it.f.name}»`; return; }
      p.set("title", t);
      if (it.yIn.value) p.set("year", it.yIn.value);
    }
    jobs.push([it, p]);
  }
  upCancel = false;
  busy(true);
  let ok = 0, bad = 0;
  try {
    for (const [i, [it, p]] of jobs.entries()) {
      if (upCancel) break;
      it.st.className = "urow-st";
      it.st.textContent = "Subiendo…";
      try {
        await uploadOne(it.f, p, token, (a, b) => {
          const pct = Math.floor((a / b) * 100);
          upBar.max = b;
          upBar.value = a;
          it.st.textContent = pct + "%";
          upMsg.textContent = `Archivo ${i + 1} de ${jobs.length} · ${pct}% · ${fmtSize(a)} de ${fmtSize(b)}`;
        });
        it.done = true;
        it.st.className = "urow-st done";
        it.st.textContent = "Subido";
        ok++;
      } catch (err) {
        if (err.aborted) { it.st.textContent = "Cancelado"; break; }
        it.st.className = "urow-st bad";
        it.st.textContent = err.message;
        bad++;
        if (err.status === 401 || err.status === 403) break; // clave incorrecta: no tiene sentido seguir
      }
    }
  } finally {
    busy(false);
  }
  const left = jobs.filter(([it]) => !it.done).length;
  upMsg.className = bad || left ? "msg" : "msg ok";
  upMsg.textContent = `${ok} subido${ok === 1 ? "" : "s"}${bad ? `, ${bad} con error` : ""}${left && !bad ? `, ${left} pendiente${left === 1 ? "" : "s"}` : ""}.` +
    (left ? " Puedes corregir y volver a pulsar Subir: solo se envían los que faltan." : "");
  if (ok) { load(); refreshShows(); }
  if (!left) {
    const keep = serie && !showId ? name : "";
    clearPick();
    $("up-name").value = keep;
    syncUp();
  } else {
    batch.items = batch.items.filter((it) => !it.done);
    renderRows();
    syncUp();
  }
}

upForm.addEventListener("submit", async (e) => {
  e.preventDefault();
  if (upBusy) { upCancel = true; if (upXhr) upXhr.abort(); return; } // el botón hace de «Cancelar»
  if (!upPicked.length) { upMsg.className = "msg"; upMsg.textContent = "Elige uno o varios videos, o una carpeta"; return; }
  upMsg.className = "msg";
  upMsg.textContent = "";
  const token = upToken.value;
  if (batch) await submitBatch(token); else await submitSingle(token);
});

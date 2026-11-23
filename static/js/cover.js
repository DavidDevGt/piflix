"use strict";
/* ---------------- título y carátula puestos a mano (películas y series) ----------------
   El navegador recorta la imagen a formato póster 2:3 y la reduce a 400×600 JPEG antes de enviarla. */
const cvForm = $("eps-cover"), cvFile = $("cv-file"), cvTok = $("cv-token"), cvMsg = $("cv-msg"), cvCan = $("cv-canvas"), cvGo = $("cv-go");
let cvShow = null, cvBlob = null;
try { cvTok.value = localStorage.getItem(TOKEN_KEY) || ""; } catch {}

const cvTitle = $("cv-title");
const cvChanged = () => !!cvBlob || (cvShow && cvTitle.value.trim() !== cvShow.title);
const cvSync = () => { cvGo.disabled = !cvChanged(); };
cvTitle.addEventListener("input", cvSync);

const cvSay = (t, ok) => { cvMsg.className = ok ? "msg ok" : "msg"; cvMsg.textContent = t; };

const edDlg = $("ed");

/* Abre el editor para una película o una serie */
function openEdit(it) {
  cvShow = it;
  cvBlob = null;
  cvFile.value = "";
  cvSay("");
  cvCan.getContext("2d").clearRect(0, 0, cvCan.width, cvCan.height);
  const serie = isShow(it);
  $("ed-h").textContent = serie ? "Editar serie" : "Editar película";
  $("cv-title-l").textContent = serie ? "Título de la serie" : "Título de la película";
  $("cv-hint").hidden = it.has_poster;
  cvTitle.value = it.title;
  cvSync();
  edDlg.showModal();
  cvTitle.focus();
  cvTitle.select();
}
$("ed-close").addEventListener("click", () => edDlg.close());
edDlg.addEventListener("click", (e) => { if (e.target === edDlg) edDlg.close(); });
$("eps-cover-btn").addEventListener("click", () => epsShow && openEdit(epsShow));

cvFile.addEventListener("change", async () => {
  const f = cvFile.files[0];
  cvBlob = null;
  cvSync();
  cvSay("");
  if (!f) return;
  if (!f.type.startsWith("image/")) return cvSay("Elige un archivo de imagen");
  try {
    const bmp = await createImageBitmap(f);
    const W = cvCan.width, H = cvCan.height, g = cvCan.getContext("2d");
    const k = Math.max(W / bmp.width, H / bmp.height); // «cover»: llena el póster y recorta lo que sobre
    g.fillStyle = "#141419"; // los PNG con transparencia no quedan negros
    g.fillRect(0, 0, W, H);
    g.drawImage(bmp, (W - bmp.width * k) / 2, (H - bmp.height * k) / 2, bmp.width * k, bmp.height * k);
    bmp.close();
    cvBlob = await new Promise((r) => cvCan.toBlob(r, "image/jpeg", 0.88));
    cvSync();
    cvSay("Se recorta al centro en formato póster (2:3).", true);
  } catch {
    cvSay("No se pudo leer esa imagen");
  }
});

cvForm.addEventListener("submit", async (e) => {
  e.preventDefault();
  if (!cvShow || !cvChanged()) return;
  const show = cvShow, title = cvTitle.value.trim(), headers = { "x-admin-token": cvTok.value };
  cvGo.disabled = true;
  cvGo.textContent = "Guardando…";
  const call = async (url, opts) => {
    const r = await fetch(url, { method: "POST", ...opts, headers: { ...headers, ...opts.headers } });
    const d = await r.json().catch(() => ({}));
    if (!r.ok) throw new Error(d.error || `El servidor respondió ${r.status}`);
    return d;
  };
  let note = "";
  try {
    if (cvBlob) {
      await call(`/api/cover/${show.id}`, { headers: { "content-type": "image/jpeg" }, body: cvBlob });
      try { localStorage.setItem(TOKEN_KEY, cvTok.value); } catch {}
      cvBlob = null;
      cvFile.value = "";
      show.has_poster = true;
    }
    if (title !== show.title) {
      const hadPoster = show.has_poster;
      const d = await call(`/api/title/${show.id}`, { headers: { "content-type": "application/json" }, body: JSON.stringify({ title }) });
      try { localStorage.setItem(TOKEN_KEY, cvTok.value); } catch {}
      if (title) show.title = title; // vacío = original: lo trae load()
      if (!hadPoster && d.tmdb) {
        note = d.has_poster ? " · carátula encontrada en TMDB" : d.tmdb_error ? " · no se pudo consultar TMDB" : " · TMDB no encontró carátula con ese título";
      }
    }
    cvSay("Cambios guardados" + note, true);
    await load(); // tarjetas con el título y la imagen nuevos
    if (epsDlg.open && epsShow && epsShow.id === show.id) { // ficha de serie abierta debajo
      $("eps-title").textContent = show.title;
      if (!epsShow.has_backdrop) {
        $("eps-img").src = `/api/img/${show.id}/poster?v=${Date.now()}`;
        $("eps-img").hidden = false;
        $("eps-head").classList.remove("no-img");
      }
    }
  } catch (err) {
    cvSay(err.message);
  } finally {
    cvGo.textContent = "Guardar cambios";
    cvSync();
  }
});

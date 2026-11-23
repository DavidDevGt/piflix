"use strict";
/* Resume: posiciones y «visto» guardados en este navegador (localStorage). Sin dependencias del DOM.
   pos:  id -> [segundos, fecha, duración]      seen: [id, …] en orden de visionado */
const Resume = (() => {
  const POS_KEY = "piflix-pos", SEEN_KEY = "piflix-seen";
  const POS_MAX = 200, SEEN_MAX = 3000, MIN_START = 10, MIN_LEFT = 30;
  const read = (key, fallback) => { try { return JSON.parse(localStorage.getItem(key)) ?? fallback; } catch { return fallback; } };
  const write = (key, value) => { try { localStorage.setItem(key, JSON.stringify(value)); } catch {} };
  /* ¿queda algo que retomar? (ni al inicio ni casi al final) */
  const worth = (t, duration) => t >= MIN_START && duration - t >= MIN_LEFT;

  const api = {
    worth,
    all: () => read(POS_KEY, {}),
    get: (id) => read(POS_KEY, {})[id] || null,
    clear(id) {
      const all = read(POS_KEY, {});
      if (!(id in all)) return;
      delete all[id];
      write(POS_KEY, all);
    },
    save(id, t, duration) {
      const all = read(POS_KEY, {});
      if (worth(t, duration)) all[id] = [Math.floor(t), Date.now(), Math.floor(duration)];
      else {
        delete all[id];
        if (t >= MIN_START) api.markSeen(id); // llegó a los créditos
      }
      const ids = Object.keys(all);
      if (ids.length > POS_MAX) for (const k of ids.sort((a, b) => all[a][1] - all[b][1]).slice(0, ids.length - POS_MAX)) delete all[k];
      write(POS_KEY, all);
    },
    markSeen(id) {
      const seen = read(SEEN_KEY, []);
      if (seen.includes(id)) return;
      seen.push(id);
      write(SEEN_KEY, seen.slice(-SEEN_MAX));
    },
    seen: () => new Set(read(SEEN_KEY, [])),
  };
  return api;
})();

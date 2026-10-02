// Leitura e resumo dos dados do Lichess para as funções da Vercel. A lista de partidas de um
// usuário só responde com token (em 02/10/2026 sem token dava 404 para qualquer conta); o token
// fica na variável de ambiente LICHESS_TOKEN da Vercel, nunca no navegador.

const API = "https://lichess.org";

/** Partidas do Lichess em ndjson, resumidas do lado de `me`, na ordem recebida. */
export function summarizeGames(ndjson, me) {
  const id = me.toLowerCase();
  const games = [];
  for (const line of ndjson.split("\n")) {
    if (!line.trim()) continue;
    const g = JSON.parse(line);
    if (!g.players) continue;
    const color = g.players.white?.user?.id === id ? "white" : "black";
    const mine = g.players[color];
    const other = g.players[color === "white" ? "black" : "white"];
    const result = !g.winner ? "draw" : g.winner === color ? "win" : "loss";
    const clock = g.clock ? `${g.clock.initial / 60}+${g.clock.increment}` : g.speed;
    games.push({
      id: g.id,
      result,
      color,
      opponent: { name: other.user?.name ?? "Anonymous", rating: other.rating ?? null, title: other.user?.title ?? null },
      rating: mine.rating ?? null,
      speed: g.speed,
      clock,
      rated: !!g.rated,
      status: g.status,
      at: g.createdAt,
    });
  }
  return games;
}

/** Vitórias, empates e derrotas numa lista resumida (a mais recente primeiro em `last`). */
export function record(games) {
  const count = (r) => games.filter((g) => g.result === r).length;
  return {
    wins: count("win"),
    draws: count("draw"),
    losses: count("loss"),
    games: games.length,
    last: games.slice(0, 5).map((g) => g.result),
  };
}

const text = (html) =>
  html
    .replace(/<[^>]+>/g, " ")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&amp;/g, "&")
    .replace(/\s+/g, " ")
    .trim();

/**
 * Página de ranking do lichess-bot-leaderboard (Eirik0, MIT): colunas posição, variação da
 * posição, nome (link), bandeira, rating, variação do rating, ...
 */
export function parseLeaderboard(html) {
  const updated = html.match(/(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2} UTC)/)?.[1] ?? null;
  const rows = [];
  for (const [, row] of html.matchAll(/<tr[^>]*>([\s\S]*?)<\/tr>/g)) {
    const cells = [...row.matchAll(/<td[^>]*>([\s\S]*?)<\/td>/g)].map((m) => m[1]);
    if (cells.length < 6) continue;
    // A célula da posição pode ter uma medalha antes do número (top 3).
    const rank = Number(text(cells[0]).match(/\d+/)?.[0]);
    const name = text(cells[2].match(/<a[^>]*>([\s\S]*?)<\/a>/)?.[1] ?? cells[2].split(/\s+/).pop());
    const rating = Number(text(cells[4]));
    if (!rank || !name || !rating) continue;
    rows.push({ rank, name, rating, delta: text(cells[5]), move: text(cells[1]) });
  }
  return { updated, rows };
}

/**
 * Reordena os bots elegíveis pelos ratings atuais (`current`: id em minúsculas → rating de blitz),
 * com empates dividindo a posição (1224); quem não tem rating atual fica com o da lista.
 */
export function rerank(rows, current) {
  const updated = rows.map((r) => {
    const rating = current[r.name.toLowerCase()] ?? r.rating;
    const diff = rating - r.rating;
    // Variação desde a lista (até 2 h atrás) até agora.
    return { ...r, rating, delta: diff > 0 ? `+${diff}` : diff < 0 ? String(diff) : "" };
  });
  updated.sort((a, b) => b.rating - a.rating);
  updated.forEach((r, i) => {
    r.rank = i > 0 && r.rating === updated[i - 1].rating ? updated[i - 1].rank : i + 1;
  });
  return updated;
}

/** Rating de blitz atual de até 300 contas por consulta (POST /api/users, sem token). */
export async function currentBlitz(names) {
  const out = {};
  for (let i = 0; i < names.length; i += 300) {
    const response = await fetch(`${API}/api/users`, {
      method: "POST",
      headers: { "Content-Type": "text/plain", Accept: "application/json" },
      body: names.slice(i, i + 300).join(","),
    });
    if (!response.ok) throw new Error(`Lichess respondeu ${response.status}`);
    for (const user of await response.json()) {
      const blitz = user.perfs?.blitz;
      if (blitz && !blitz.prov) out[user.id] = blitz.rating;
    }
  }
  return out;
}

/** O topo e a vizinhança de `name` (sem repetir linhas), com `me` marcado. */
export function windowAround(rows, name, { top = 3, around = 3 } = {}) {
  const at = rows.findIndex((r) => r.name.toLowerCase() === name.toLowerCase());
  const keep = new Set(rows.slice(0, top).map((_, i) => i));
  if (at >= 0) {
    for (let i = Math.max(0, at - around); i <= Math.min(rows.length - 1, at + around); i++) keep.add(i);
  }
  return [...keep].sort((a, b) => a - b).map((i) => ({ ...rows[i], me: i === at }));
}

/** Exporta partidas de `user` (ndjson) com o token; `params` vira a query string. */
export async function fetchGames(user, params, token) {
  const query = new URLSearchParams({ moves: "false", ...params });
  const response = await fetch(`${API}/api/games/user/${encodeURIComponent(user)}?${query}`, {
    headers: { Accept: "application/x-ndjson", ...(token ? { Authorization: `Bearer ${token}` } : {}) },
  });
  if (!response.ok) throw new Error(`Lichess respondeu ${response.status}`);
  return response.text();
}

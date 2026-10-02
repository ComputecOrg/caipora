// GET /api/ranking: ranking de bots no blitz. Quem entra (regras de elegibilidade) vem do
// lichess-bot-leaderboard (Eirik0, MIT), que atualiza a cada 2 h; o rating de cada um é buscado
// agora no Lichess e a lista é reordenada. Cache de 60 s na CDN da Vercel.
import { parseLeaderboard, windowAround, rerank, currentBlitz } from "./_lib/lichess.js";

const SOURCE = "https://eirik0.github.io/lichess-bot-leaderboard/blitz.html";

export default async function handler(req, res) {
  try {
    const response = await fetch(SOURCE);
    if (!response.ok) throw new Error(`ranking respondeu ${response.status}`);
    const board = parseLeaderboard(await response.text());
    let rows = board.rows;
    let live = false;
    try {
      rows = rerank(rows, await currentBlitz(rows.map((r) => r.name)));
      live = true;
    } catch {
      // sem os ratings atuais, fica a ordem da lista
    }
    const me = rows.find((r) => r.name.toLowerCase() === "caiporabot") ?? null;
    res.setHeader("Cache-Control", "public, s-maxage=60, stale-while-revalidate=300");
    res.status(200).json({
      updated: live ? new Date().toISOString().replace("T", " ").slice(0, 16) + " UTC" : board.updated,
      eligibility: board.updated,
      live,
      total: rows.length,
      me,
      rows: windowAround(rows, "caiporaBot", { top: 3, around: 3 }),
      source: SOURCE,
    });
  } catch (error) {
    res.status(502).json({ error: error.message });
  }
}

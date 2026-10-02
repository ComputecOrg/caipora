// GET /api/ranking: o ranking de bots no blitz do lichess-bot-leaderboard (Eirik0, MIT, atualiza
// a cada 2 h), com o topo e a vizinhança do caiporaBot. Cache de 30 min na CDN da Vercel.
import { parseLeaderboard, windowAround } from "./_lib/lichess.js";

const SOURCE = "https://eirik0.github.io/lichess-bot-leaderboard/blitz.html";

export default async function handler(req, res) {
  try {
    const response = await fetch(SOURCE);
    if (!response.ok) throw new Error(`ranking respondeu ${response.status}`);
    const board = parseLeaderboard(await response.text());
    const me = board.rows.find((r) => r.name.toLowerCase() === "caiporabot") ?? null;
    res.setHeader("Cache-Control", "public, s-maxage=1800, stale-while-revalidate=7200");
    res.status(200).json({
      updated: board.updated,
      total: board.rows.length,
      me,
      rows: windowAround(board.rows, "caiporaBot", { top: 3, around: 3 }),
      source: "https://eirik0.github.io/lichess-bot-leaderboard/blitz.html",
    });
  } catch (error) {
    res.status(502).json({ error: error.message });
  }
}

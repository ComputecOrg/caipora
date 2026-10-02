// GET /api/h2h?opponent=<nome>: vitórias, empates e derrotas do caiporaBot contra um adversário
// (até 300 partidas). Cache de 5 min na CDN da Vercel.
import { fetchGames, summarizeGames, record } from "./_lib/lichess.js";

const BOT = "caiporaBot";

export default async function handler(req, res) {
  const opponent = String(req.query.opponent || "");
  if (!/^[A-Za-z0-9_-]{2,30}$/.test(opponent)) {
    res.status(400).json({ error: "adversário inválido" });
    return;
  }
  try {
    const ndjson = await fetchGames(BOT, { vs: opponent, max: "300" }, process.env.LICHESS_TOKEN);
    res.setHeader("Cache-Control", "public, s-maxage=300, stale-while-revalidate=1800");
    res.status(200).json({ opponent, ...record(summarizeGames(ndjson, BOT)) });
  } catch (error) {
    res.status(502).json({ error: error.message });
  }
}

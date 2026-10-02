// GET /api/games: as últimas partidas do caiporaBot, resumidas. Cache de 2 min na CDN da Vercel.
import { fetchGames, summarizeGames } from "./_lib/lichess.js";

const BOT = "caiporaBot";

export default async function handler(req, res) {
  try {
    const ndjson = await fetchGames(BOT, { max: "12" }, process.env.LICHESS_TOKEN);
    res.setHeader("Cache-Control", "public, s-maxage=120, stale-while-revalidate=600");
    res.status(200).json({ games: summarizeGames(ndjson, BOT) });
  } catch (error) {
    res.status(502).json({ error: error.message });
  }
}

// Resumo do fim da partida: fase de cada posição, contagem de categorias e precisão por jogador e
// por fase (mesma fórmula da precisão geral, gameAccuracy).
import { gameAccuracy } from "./analysis.js";

const PIECES = { q: 9, r: 5, b: 3, n: 3 };
export const PHASES = ["opening", "middlegame", "endgame"];
export const PHASE_LABEL = { opening: "Abertura", middlegame: "Meio-jogo", endgame: "Final" };

/** Abertura até o lance 12; final com material sem peões (dos dois lados) <= 20; meio-jogo no resto. */
export function phaseOf(fen) {
  const [board, , , , , full] = fen.split(" ");
  if (Number(full) <= 12) return "opening";
  let material = 0;
  for (const ch of board.toLowerCase()) material += PIECES[ch] || 0;
  return material <= 20 ? "endgame" : "middlegame";
}

/**
 * `moves`: [{white, category, winBefore, winAfter, phase}] (% de vitória de quem jogou).
 * Devolve, para cada lado, as contagens por categoria, a precisão geral e a de cada fase.
 */
export function summarize(moves) {
  const side = (white) => {
    const own = moves.filter((m) => m.white === white);
    const counts = {};
    for (const m of own) counts[m.category] = (counts[m.category] || 0) + 1;
    const acc = (list) => gameAccuracy(list.map((m) => ({ before: m.winBefore, after: m.winAfter })));
    const phases = {};
    for (const phase of PHASES) phases[phase] = acc(own.filter((m) => m.phase === phase));
    return { counts, accuracy: acc(own), phases };
  };
  return { white: side(true), black: side(false) };
}

/** Até 3 destaques: brilhante sempre que houver, depois ótimo, melhor, excelente e bom. */
export function highlights(counts) {
  return ["brilliant", "great", "best", "excellent", "good"].filter((key) => counts[key]).slice(0, 3);
}

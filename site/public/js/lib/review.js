// Revisão dos lances no estilo do Game Review do chess.com, a partir das análises de cada posição.
// A lógica é a do painel da extensão Caipora Live (mesmo autor), separada do estado para o site.
import { Chess } from "./chess.js";
import { expectedPoints, classifyChessCom, gameAccuracy, materialOf, winningChances, scoreToCp } from "./analysis.js";
import { uciLineToSan } from "./game.js";

export const LABELS = {
  brilliant: { label: "Brilliant", symbol: "!!", color: "#26a69a" },
  great: { label: "Great", symbol: "!", color: "#5c9ee8" },
  best: { label: "Best", symbol: "", color: "#3fa86a" },
  excellent: { label: "Excellent", symbol: "", color: "#3fa86a" },
  good: { label: "Good", symbol: "", color: "#8fb36a" },
  inaccuracy: { label: "Inaccuracy", symbol: "?!", color: "#e2b84a" },
  mistake: { label: "Mistake", symbol: "?", color: "#e0843a" },
  miss: { label: "Miss", symbol: "?", color: "#e0843a" },
  blunder: { label: "Blunder", symbol: "??", color: "#c6423c" },
};

const sideOf = (positions, i) => positions[i].fen.split(" ")[1];

function sacrifice(positions, evals, k) {
  const reply = evals[k]?.pv?.[0];
  if (!reply) return false;
  const side = sideOf(positions, k - 1);
  const other = side === "w" ? "b" : "w";
  const balance = (fen) => materialOf(fen, side) - materialOf(fen, other);
  const chess = new Chess(positions[k].fen);
  try {
    chess.move({ from: reply.slice(0, 2), to: reply.slice(2, 4), promotion: reply[4] });
  } catch {
    return false;
  }
  return balance(positions[k - 1].fen) - balance(chess.fen()) >= 2;
}

/**
 * Veredito de cada lance (índice k = lance que leva da posição k-1 à k; o índice 0 é null):
 * `{category, best, moverWhite}`, ou null sem análise com profundidade `depth` dos dois lados.
 * `evals[i]` e `seconds[i]` são a 1ª e a 2ª linha da posição i, do lado das brancas.
 */
export function classifyMoves(positions, evals, seconds, depth) {
  const deep = (ev) => ev && ev.depth >= depth;
  const loss = [0];
  const verdicts = [null];
  for (let k = 1; k < positions.length; k++) {
    const before = evals[k - 1];
    const after = evals[k];
    if (!deep(before) || !deep(after)) {
      loss.push(null);
      verdicts.push(null);
      continue;
    }
    const white = sideOf(positions, k - 1) === "w";
    const b = expectedPoints(before.score, white);
    const a = expectedPoints(after.score, white);
    loss.push(Math.max(0, b - a));
    const category = classifyChessCom({
      before: b,
      after: a,
      second: deep(seconds[k - 1]) ? expectedPoints(seconds[k - 1].score, white) : null,
      playedIsBest: before.pv?.[0] === positions[k].uci,
      prevLoss: loss[k - 1] ?? 0,
      sacrifice: sacrifice(positions, evals, k),
    });
    const best = uciLineToSan(positions[k - 1].fen, before.pv || [], 1)[0] ?? null;
    verdicts.push({ category, best, moverWhite: white });
  }
  return verdicts;
}

/** Precisão de cada lado (fórmula aberta do Lichess) com as posições já analisadas. */
export function accuracies(positions, evals, depth) {
  const moves = { w: [], b: [] };
  for (let k = 1; k < positions.length; k++) {
    const before = evals[k - 1];
    const after = evals[k];
    if (!before || !after || before.depth < depth || after.depth < depth) continue;
    const side = sideOf(positions, k - 1);
    const sign = side === "w" ? 1 : -1;
    const win = (score) => 50 + 50 * winningChances(sign * scoreToCp(score));
    moves[side].push({ before: win(before.score), after: win(after.score) });
  }
  return { white: gameAccuracy(moves.w), black: gameAccuracy(moves.b) };
}

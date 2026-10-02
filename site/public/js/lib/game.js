// Posições de uma partida a partir dos lances (usa chess.js, licença BSD-2).
import { Chess } from "./chess.js";

/**
 * Lances em SAN (ou UCI) viram a lista de posições: a inicial e uma depois de cada lance, com o
 * lance em SAN e em UCI. Um lance inválido encerra a lista ali.
 */
export function buildPositions(moves, startFen) {
  const chess = startFen ? new Chess(startFen) : new Chess();
  const positions = [{ fen: chess.fen(), san: null, uci: null }];
  for (const move of moves) {
    let played;
    try {
      played = chess.move(toMoveArg(move));
    } catch {
      break;
    }
    positions.push({
      fen: chess.fen(),
      san: played.san,
      uci: played.from + played.to + (played.promotion || ""),
    });
  }
  return positions;
}

/** Converte uma variante em UCI para SAN, no máximo `limit` lances; para no primeiro inválido. */
export function uciLineToSan(fen, uciMoves, limit) {
  const chess = new Chess(fen);
  const out = [];
  for (const uci of uciMoves.slice(0, limit)) {
    try {
      out.push(chess.move(toMoveArg(uci)).san);
    } catch {
      break;
    }
  }
  return out;
}

/** Aceita SAN ("Nf3") ou UCI ("g1f3", "e7e8q"). */
function toMoveArg(move) {
  if (/^[a-h][1-8][a-h][1-8][qrbn]?$/.test(move)) {
    return { from: move.slice(0, 2), to: move.slice(2, 4), promotion: move[4] };
  }
  return move;
}

// Leitura do fluxo ndjson de uma partida do Lichess (/api/stream/game/{id}): a primeira linha
// descreve a partida; as seguintes trazem a FEN, o último lance (UCI) e os relógios, a partir da
// posição inicial.
import { Chess } from "./chess.js";

/** Junta pedaços de texto e devolve só as linhas JSON completas. */
export class NdjsonSplitter {
  constructor() {
    this.buffer = "";
  }

  push(chunk) {
    this.buffer += chunk;
    const lines = this.buffer.split("\n");
    this.buffer = lines.pop();
    return lines.filter((l) => l.trim()).map((l) => JSON.parse(l));
  }
}

/** Eventos do fluxo → posição inicial, lances em SAN e relógios (segundos) da última posição. */
export function movesFromStream(events) {
  const positions = events.filter((e) => e.fen);
  if (!positions.length) return { startFen: null, moves: [], clock: null };
  const startFen = positions[0].fen;
  const chess = new Chess(startFen);
  const moves = [];
  for (const e of positions.slice(1)) {
    if (!e.lm) continue;
    try {
      const m = chess.move({ from: e.lm.slice(0, 2), to: e.lm.slice(2, 4), promotion: e.lm[4] || undefined });
      moves.push(m.san);
    } catch {
      // Roque no formato "rei captura torre" (e1h1): converte para o destino padrão.
      const castle = { e1h1: "e1g1", e1a1: "e1c1", e8h8: "e8g8", e8a8: "e8c8" }[e.lm];
      if (!castle) break;
      moves.push(chess.move({ from: castle.slice(0, 2), to: castle.slice(2, 4) }).san);
    }
  }
  const last = positions[positions.length - 1];
  return { startFen, moves, clock: { white: last.wc, black: last.bc } };
}

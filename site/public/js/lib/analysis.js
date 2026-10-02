// Lógica pura da análise: leitura das linhas do Stockfish, chance de vitória e classificação dos
// lances com os mesmos limites do Lichess.

/** Chance de vitória em [-1, 1] a partir de centipeões (curva do Lichess). */
export function winningChances(cp) {
  return 2 / (1 + Math.exp(-0.00368208 * cp)) - 1;
}

/** Converte {cp} ou {mate} em centipeões; mates mais curtos valem mais. */
export function scoreToCp(score) {
  if (score.mate !== undefined) {
    const sign = score.mate > 0 ? 1 : -1;
    return sign * (10000 - Math.abs(score.mate) * 10);
  }
  return score.cp;
}

/**
 * Lê uma linha `info` do Stockfish. A pontuação vem do lado a jogar; devolve do lado das brancas.
 * Linhas sem pontuação final (currmove, string, lowerbound/upperbound) devolvem null.
 */
export function parseInfo(line, whiteToMove) {
  if (!line.startsWith("info ") || line.includes(" lowerbound") || line.includes(" upperbound")) {
    return null;
  }
  const tokens = line.split(/\s+/);
  const at = (name) => tokens.indexOf(name);
  const scoreAt = at("score");
  const pvAt = at("pv");
  if (scoreAt < 0 || pvAt < 0 || at("depth") < 0) {
    return null;
  }
  const kind = tokens[scoreAt + 1];
  const value = Number(tokens[scoreAt + 2]);
  const sign = whiteToMove ? 1 : -1;
  const score = kind === "mate" ? { mate: sign * value } : { cp: sign * value };
  const npsAt = at("nps");
  const multipvAt = at("multipv");
  return {
    depth: Number(tokens[at("depth") + 1]),
    multipv: multipvAt >= 0 ? Number(tokens[multipvAt + 1]) : 1,
    score,
    nps: npsAt >= 0 ? Number(tokens[npsAt + 1]) : 0,
    pv: tokens.slice(pvAt + 1).filter(Boolean),
  };
}

/**
 * Classifica o lance pela perda de chance de vitória de quem jogou (limites do Lichess: 0,1
 * imprecisão, 0,2 erro, 0,3 erro grave). `before` e `after` são do lado das brancas.
 */
export function judge(before, after, moverIsWhite) {
  const sign = moverIsWhite ? 1 : -1;
  const loss =
    winningChances(sign * scoreToCp(before)) - winningChances(sign * scoreToCp(after));
  if (loss >= 0.3) return "blunder";
  if (loss >= 0.2) return "mistake";
  if (loss >= 0.1) return "inaccuracy";
  return null;
}

/** Avaliação no formato do Lichess: +0.8, -8.9, #4, #-22. */
export function formatScore(score) {
  if (score.mate !== undefined) {
    return `#${score.mate}`;
  }
  const pawns = score.cp / 100;
  const text = pawns.toFixed(1);
  if (text === "0.0" || text === "-0.0") return "0.0";
  return pawns > 0 ? `+${text}` : text;
}

export const JUDGEMENT = {
  inaccuracy: { symbol: "?!", label: "Imprecisão" },
  mistake: { symbol: "?", label: "Erro" },
  blunder: { symbol: "??", label: "Erro grave" },
};

// ---------- estilo chess.com ----------

/** Pontos esperados (0 a 1) de quem joga; curva do Lichess (a do chess.com depende do rating). */
export function expectedPoints(score, forWhite) {
  const sign = forWhite ? 1 : -1;
  return (winningChances(sign * scoreToCp(score)) + 1) / 2;
}

/**
 * Categoria do lance no estilo do Game Review do chess.com, pela perda de pontos esperados
 * (limites publicados por eles: 0,02 / 0,05 / 0,10 / 0,20). `before` é o melhor lance, `after` o
 * jogado e `second` o segundo melhor, todos do lado de quem joga; `prevLoss` é a perda do lance
 * anterior do adversário; `sacrifice` diz se a melhor resposta ganha material (>= 2).
 */
export function classifyChessCom({ before, after, second, playedIsBest, prevLoss, sacrifice }) {
  const loss = Math.max(0, before - after);
  if (loss <= 0.02 && sacrifice && before < 0.95 && after >= 0.5) return "brilliant";
  if (playedIsBest && second !== null && second !== undefined && before - second >= 0.1) return "great";
  if (playedIsBest || loss === 0) return "best";
  if (loss <= 0.02) return "excellent";
  if (loss <= 0.05) return "good";
  if (loss >= 0.1 && prevLoss >= 0.1) return "miss";
  if (loss <= 0.1) return "inaccuracy";
  if (loss <= 0.2) return "mistake";
  return "blunder";
}

export const CHESSCOM = {
  brilliant: { label: "Brilhante", symbol: "!!", bad: false },
  great: { label: "Ótimo", symbol: "!", bad: false },
  best: { label: "Melhor", symbol: "", bad: false },
  excellent: { label: "Excelente", symbol: "", bad: false },
  good: { label: "Bom", symbol: "", bad: false },
  inaccuracy: { label: "Imprecisão", symbol: "?!", bad: true },
  mistake: { label: "Erro", symbol: "?", bad: true },
  miss: { label: "Perdeu a chance", symbol: "?", bad: true },
  blunder: { label: "Erro grave", symbol: "??", bad: true },
};

/** Precisão de um lance (0 a 100) pela perda de % de vitória; fórmula aberta do Lichess. */
export function moveAccuracy(winBefore, winAfter) {
  const drop = Math.max(0, winBefore - winAfter);
  const raw = 103.1668 * Math.exp(-0.04354 * drop) - 3.1669;
  return Math.min(100, Math.max(0, raw + 1));
}

/**
 * Precisão da partida de um jogador: média ponderada pela volatilidade e média harmônica, como o
 * Lichess. `moves` = [{before, after}] em % de vitória de quem joga.
 */
export function gameAccuracy(moves) {
  if (!moves.length) return null;
  const accs = moves.map((m) => moveAccuracy(m.before, m.after));
  const wins = moves.map((m) => m.before);
  const size = Math.min(8, Math.max(2, Math.floor(moves.length / 10)));
  const weights = accs.map((_, i) => {
    const window = wins.slice(Math.max(0, i - size + 1), i + 1);
    const mean = window.reduce((a, b) => a + b, 0) / window.length;
    const sd = Math.sqrt(window.reduce((a, b) => a + (b - mean) ** 2, 0) / window.length);
    return Math.min(12, Math.max(0.5, sd));
  });
  const weighted = accs.reduce((a, acc, i) => a + acc * weights[i], 0) / weights.reduce((a, b) => a + b, 0);
  const harmonic = accs.length / accs.reduce((a, acc) => a + 1 / Math.max(acc, 1), 0);
  return (weighted + harmonic) / 2;
}

const VALUES = { p: 1, n: 3, b: 3, r: 5, q: 9, k: 0 };

/** Material (1/3/3/5/9) de um lado ("w" ou "b") numa FEN. */
export function materialOf(fen, side) {
  let total = 0;
  for (const ch of fen.split(" ")[0]) {
    const lower = ch.toLowerCase();
    if (!(lower in VALUES)) continue;
    const isWhite = ch !== lower;
    if ((side === "w") === isWhite) total += VALUES[lower];
  }
  return total;
}

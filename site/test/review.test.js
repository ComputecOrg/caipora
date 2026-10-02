import { test } from "node:test";
import assert from "node:assert/strict";
import { buildPositions } from "../public/js/lib/game.js";
import { classifyMoves, accuracies, gameSummary, terminalEval, LABELS } from "../public/js/lib/review.js";
import { NdjsonSplitter, movesFromStream, gameResult, formatDiff } from "../public/js/lib/stream.js";

const deep = (cp, pv = []) => ({ depth: 20, score: { cp }, pv });

test("each move is classified from the evaluations before and after it", () => {
  const positions = buildPositions(["e4", "e5", "Qh5"]);
  // Avaliações do lado das brancas; 1...e5 perde quase nada, 2.Qh5 joga fora uma vantagem grande.
  const evals = [deep(30, ["e2e4"]), deep(35, ["e7e5"]), deep(30, ["g1f3"]), deep(-400, ["g8f6"])];
  const seconds = [deep(20), deep(20), deep(20), deep(-420)];
  const verdicts = classifyMoves(positions, evals, seconds, 18);
  assert.equal(verdicts[0], null); // posição inicial não é lance
  assert.equal(verdicts[1].category, "best");
  assert.equal(verdicts[1].moverWhite, true);
  assert.equal(verdicts[3].category, "blunder");
  assert.equal(verdicts[3].best, "Nf3");
});

test("moves without deep enough analysis stay unclassified", () => {
  const positions = buildPositions(["e4"]);
  const shallow = { depth: 5, score: { cp: 30 }, pv: [] };
  assert.equal(classifyMoves(positions, [shallow, shallow], [], 18)[1], null);
});

test("accuracy is computed per side", () => {
  const positions = buildPositions(["e4", "e5"]);
  const evals = [deep(30), deep(30), deep(30)];
  const acc = accuracies(positions, evals, 18);
  assert.ok(acc.white > 95 && acc.black > 95, JSON.stringify(acc));
});

test("labels are in English for the site", () => {
  assert.equal(LABELS.blunder.label, "Blunder");
  assert.equal(LABELS.brilliant.label, "Brilliant");
  assert.equal(LABELS.best.label, "Best");
});

test("the ndjson splitter returns whole objects across chunk boundaries", () => {
  const splitter = new NdjsonSplitter();
  assert.deepEqual(splitter.push('{"a":1}\n{"b"'), [{ a: 1 }]);
  assert.deepEqual(splitter.push(':2}\n\n'), [{ b: 2 }]);
});

test("stream events become the move list in SAN with clocks", () => {
  const events = [
    { id: "x", players: {} },
    { fen: "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", wc: 180, bc: 180 },
    { fen: "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1", lm: "e2e4", wc: 179, bc: 180 },
    { fen: "rnbqkbnr/pp1ppppp/8/2p5/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2", lm: "c7c5", wc: 179, bc: 177 },
  ];
  const game = movesFromStream(events);
  assert.equal(game.startFen, "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
  assert.deepEqual(game.moves, ["e4", "c5"]);
  assert.deepEqual(game.clock, { white: 179, black: 177 });
});

test("the game summary has accuracy, phases and counts per side, and how much was analysed", () => {
  const positions = buildPositions(["e4", "e5", "Qh5"]);
  const evals = [deep(30, ["e2e4"]), deep(35, ["e7e5"]), deep(30, ["g1f3"]), deep(-400, ["g8f6"])];
  const s = gameSummary(positions, evals, [], 18);
  assert.deepEqual({ analysed: s.analysed, total: s.total }, { analysed: 3, total: 3 });
  assert.equal(s.white.counts.blunder, 1);
  assert.equal(s.black.counts.best, 1);
  assert.ok(s.white.accuracy < s.black.accuracy, JSON.stringify([s.white.accuracy, s.black.accuracy]));
  assert.ok(s.white.phases.opening !== undefined);
  // Lances sem análise ficam fora e aparecem na contagem.
  const partial = gameSummary(positions, [evals[0], evals[1]], [], 18);
  assert.deepEqual({ analysed: partial.analysed, total: partial.total }, { analysed: 1, total: 3 });
});

test("finished positions get their known evaluation, so the engine never has to analyse them", () => {
  // Mate do pastor: as pretas, a jogar, levaram mate.
  const mated = "r1bqkb1r/pppp1Qpp/2n2n2/4p3/2B1P3/8/PPPP1PPP/RNB1K1NR b KQkq - 0 4";
  assert.deepEqual(terminalEval(mated), { depth: Infinity, score: { cp: 10000 }, pv: [] });
  // Afogamento: empate.
  assert.deepEqual(terminalEval("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1"), { depth: Infinity, score: { cp: 0 }, pv: [] });
  // Posição normal: nada.
  assert.equal(terminalEval("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"), null);
});

test("the clocks come from the last event that has them (the closing event has none)", () => {
  const start = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
  const events = [
    { id: "x", fen: start },
    { fen: "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1", lm: "e2e4", wc: 178, bc: 180 },
    { fen: "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1" },
  ];
  assert.deepEqual(movesFromStream(events).clock, { white: 178, black: 180 });
});

test("a stream without clocks has no clock", () => {
  const start = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
  assert.equal(movesFromStream([{ fen: start }]).clock, null);
});

test("the finished game result carries the rating change of each side", () => {
  const data = { winner: "white", status: "mate", players: { white: { rating: 2899, ratingDiff: 4 }, black: { rating: 2989, ratingDiff: -4 } } };
  assert.deepEqual(gameResult(data), { winner: "white", status: "mate", ratingDiff: { white: 4, black: -4 } });
  const casual = { status: "draw", players: { white: { rating: 2899 }, black: { rating: 2989 } } };
  assert.deepEqual(gameResult(casual), { winner: null, status: "draw", ratingDiff: null });
});

test("rating changes are written with their sign", () => {
  assert.equal(formatDiff(4), "+4");
  assert.equal(formatDiff(-3), "−3");
  assert.equal(formatDiff(0), "±0");
  assert.equal(formatDiff(undefined), "");
});

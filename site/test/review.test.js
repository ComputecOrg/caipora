import { test } from "node:test";
import assert from "node:assert/strict";
import { buildPositions } from "../public/js/lib/game.js";
import { classifyMoves, accuracies, LABELS } from "../public/js/lib/review.js";
import { NdjsonSplitter, movesFromStream } from "../public/js/lib/stream.js";

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

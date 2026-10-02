import { test } from "node:test";
import assert from "node:assert/strict";
import { splitOpening } from "../public/js/lib/opening.js";

test("the opening name splits into the opening and the defence or variation", () => {
  assert.deepEqual(splitOpening({ eco: "C70", name: "Ruy Lopez: Morphy Defense, Classical Defense Deferred" }), {
    eco: "C70",
    opening: "Ruy Lopez",
    variation: "Morphy Defense, Classical Defense Deferred",
  });
});

test("a name without a variation has only the opening", () => {
  assert.deepEqual(splitOpening({ eco: "B00", name: "Borg Defense" }), { eco: "B00", opening: "Borg Defense", variation: null });
});

test("no opening data gives nothing to show", () => {
  assert.equal(splitOpening(null), null);
  assert.equal(splitOpening({ eco: "A00" }), null);
});

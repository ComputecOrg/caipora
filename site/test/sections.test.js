import { test } from "node:test";
import assert from "node:assert/strict";
import { activeSection } from "../public/js/lib/sections.js";

const tops = [0, 600, 1400, 2600];

test("before the first section, the first one is active", () => {
  assert.equal(activeSection(tops, -50, 80), 0);
});

test("the active section is the last one whose top has passed the bar", () => {
  assert.equal(activeSection(tops, 0, 80), 0);
  assert.equal(activeSection(tops, 519, 80), 0);
  assert.equal(activeSection(tops, 520, 80), 1);
  assert.equal(activeSection(tops, 2000, 80), 2);
});

test("at the bottom of the page the last section is active even if its top has not reached the bar", () => {
  assert.equal(activeSection(tops, 2300, 80, true), 3);
});

test("an empty list has no active section", () => {
  assert.equal(activeSection([], 100, 80), -1);
});

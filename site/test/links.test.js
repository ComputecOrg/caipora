import { test } from "node:test";
import assert from "node:assert/strict";
import { profileUrl } from "../public/js/lib/links.js";

test("a player name links to its Lichess profile", () => {
  assert.equal(profileUrl("little-0"), "https://lichess.org/@/little-0");
  assert.equal(profileUrl("Chess-Dragon1"), "https://lichess.org/@/Chess-Dragon1");
});

test("odd characters in a name are escaped and an empty name has no link", () => {
  assert.equal(profileUrl("a b/c"), "https://lichess.org/@/a%20b%2Fc");
  assert.equal(profileUrl(""), null);
  assert.equal(profileUrl(undefined), null);
});

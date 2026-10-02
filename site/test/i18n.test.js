import { test } from "node:test";
import assert from "node:assert/strict";
import { pickLanguage } from "../public/js/lib/i18n.js";

test("the saved choice wins, then the browser language, then English", () => {
  assert.equal(pickLanguage("en", ["pt-BR"]), "en");
  assert.equal(pickLanguage("pt", ["en-US"]), "pt");
  assert.equal(pickLanguage(null, ["pt-BR", "en"]), "pt");
  assert.equal(pickLanguage(null, ["pt"]), "pt");
  assert.equal(pickLanguage(null, ["es-ES", "en-GB"]), "en");
  assert.equal(pickLanguage("xx", []), "en");
});

import { test } from "node:test";
import assert from "node:assert/strict";
import { summarizeGames, record, parseLeaderboard, windowAround } from "../api/_lib/lichess.js";

// Duas partidas no formato ndjson da exportação do Lichess (só os campos usados).
const NDJSON = [
  {
    id: "aaaa1111",
    rated: true,
    speed: "blitz",
    createdAt: 1790939985642,
    status: "mate",
    winner: "black",
    clock: { initial: 180, increment: 2 },
    players: {
      white: { user: { name: "LinsOfBots", title: "BOT", id: "linsofbots" }, rating: 2783 },
      black: { user: { name: "caiporaBot", title: "BOT", id: "caiporabot" }, rating: 2891 },
    },
  },
  {
    id: "bbbb2222",
    rated: true,
    speed: "blitz",
    createdAt: 1790930000000,
    status: "draw",
    clock: { initial: 180, increment: 0 },
    players: {
      white: { user: { name: "caiporaBot", title: "BOT", id: "caiporabot" }, rating: 2880 },
      black: { user: { name: "Noel-bot", title: "BOT", id: "noel-bot" }, rating: 2895 },
    },
  },
].map((g) => JSON.stringify(g)).join("\n");

test("games are summarised from the bot's side", () => {
  const games = summarizeGames(NDJSON, "caiporaBot");
  assert.equal(games.length, 2);
  assert.deepEqual(games[0], {
    id: "aaaa1111",
    result: "win",
    color: "black",
    opponent: { name: "LinsOfBots", rating: 2783, title: "BOT" },
    rating: 2891,
    speed: "blitz",
    clock: "3+2",
    rated: true,
    status: "mate",
    at: 1790939985642,
  });
  assert.equal(games[1].result, "draw");
  assert.equal(games[1].color, "white");
});

test("a head-to-head record counts wins, draws and losses", () => {
  const games = summarizeGames(NDJSON, "caiporaBot");
  assert.deepEqual(record(games), { wins: 1, draws: 1, losses: 0, games: 2, last: ["win", "draw"] });
});

test("blank lines and an error object do not break the summary", () => {
  assert.deepEqual(summarizeGames('\n{"error":"Not found"}\n', "caiporaBot"), []);
});

const PAGE = `<html><body><p>Updated 2026-10-02 05:37:18 UTC</p><table>
<tr><th>#</th><th>Δ</th><th>Name</th><th>⚑</th><th>Rating</th><th>Δ</th><th>RD</th><th>Games</th></tr>
<tr><td><span class="col-rank-medal">🥇</span>1</td><td></td><td><span>●</span> BOT <a href="https://lichess.org/@/NeuroSoCute">NeuroSoCute</a></td><td></td><td>3073</td><td>+3</td><td>45</td><td>900</td></tr>
<tr><td>2</td><td>↑1</td><td>BOT <a href="x">Selimbabapro-Bot</a></td><td>🇹🇷</td><td>3063</td><td></td><td>60</td><td>23</td></tr>
<tr><td>109</td><td>↑7</td><td>BOT <a href="x">caiporaBot</a></td><td>🇧🇷</td><td>2884</td><td>+20</td><td>45</td><td>405</td></tr>
<tr><td>110</td><td>↓2</td><td>BOT <a href="x">SykoraBot</a></td><td>🇺🇸</td><td>2882</td><td>-4</td><td>45</td><td>300</td></tr>
</table></body></html>`;

test("the leaderboard page becomes rows with rank, name, rating and change", () => {
  const board = parseLeaderboard(PAGE);
  assert.equal(board.updated, "2026-10-02 05:37:18 UTC");
  assert.equal(board.rows.length, 4);
  assert.deepEqual(board.rows[0], { rank: 1, name: "NeuroSoCute", rating: 3073, delta: "+3", move: "" });
  assert.deepEqual(board.rows[2], { rank: 109, name: "caiporaBot", rating: 2884, delta: "+20", move: "↑7" });
});

test("the ranking window keeps the top and the bot's neighbourhood", () => {
  const rows = Array.from({ length: 200 }, (_, i) => ({ rank: i + 1, name: `bot${i + 1}`, rating: 3100 - i }));
  rows[108].name = "caiporaBot";
  const view = windowAround(rows, "caiporabot", { top: 3, around: 2 });
  assert.deepEqual(view.map((r) => r.rank), [1, 2, 3, 107, 108, 109, 110, 111]);
  assert.equal(view.find((r) => r.me).name, "caiporaBot");
  // Bot fora da lista: só o topo.
  assert.deepEqual(windowAround(rows, "nobody", { top: 3, around: 2 }).map((r) => r.rank), [1, 2, 3]);
});

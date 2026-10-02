// Caipora Live: a partida do caiporaBot ao vivo (fluxo público do Lichess), analisada no navegador
// de quem visita pelo Stockfish, com o histórico contra o adversário, o ranking dos bots, a evolução
// de força e as partidas recentes (com revisão).
import { Chessground } from "../vendor/chessground/chessground.js";
import { formatScore, winningChances, scoreToCp } from "./lib/analysis.js";
import { buildPositions, uciLineToSan } from "./lib/game.js";
import { nextJob, FILL_DEPTH } from "./lib/schedule.js";
import { classifyMoves, accuracies, gameSummary, terminalEval, LABELS } from "./lib/review.js";
import { NdjsonSplitter, movesFromStream, gameResult, formatDiff } from "./lib/stream.js";
import { profileUrl } from "./lib/links.js";
import { splitOpening } from "./lib/opening.js";
import { Engine } from "./engine.js";

const BOT = "caiporaBot";
const LICHESS = "https://lichess.org";
const JUDGE_DEPTH = 14;
const $ = (id) => document.getElementById(id);

// ---------- estado ----------
const game = {
  mode: "idle", // idle | live | review
  id: null,
  white: null, // {name, rating, title}
  black: null,
  botColor: "white",
  startFen: undefined,
  positions: buildPositions([]),
  evals: [],
  seconds: [],
  clock: null, // {white, black, at, running: "white"|"black"|null}
  result: null, // {winner: "white"|"black"|null, status} quando a partida acabou
  speed: "",
  tournament: null, // id do torneio da partida, se for de torneio
  opening: null, // {eco, opening, variation} pelo nome que o Lichess dá
};
// Cartão de resumo do fim da partida: aberto sozinho quando a partida ao vivo acaba; "Close" fecha.
let summaryOpen = false;
let selected = null; // null = acompanha a última posição
let streamAbort = null;
// Partida em revisão (id) ou null: marcada antes de qualquer espera, para o ao vivo não sobrepor.
let reviewing = null;

// ---------- utilidades de DOM ----------
function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "class") node.className = v;
    else if (k === "text") node.textContent = v;
    else if (k.startsWith("on")) node.addEventListener(k.slice(2), v);
    else node.setAttribute(k, v);
  }
  for (const child of children) if (child != null) node.append(child);
  return node;
}

async function getJson(url) {
  const response = await fetch(url, { headers: { Accept: "application/json" } });
  if (!response.ok) throw new Error(`${url}: ${response.status}`);
  return response.json();
}

// ---------- tabuleiro ----------
const ground = Chessground($("board"), {
  viewOnly: true,
  coordinates: true,
  animation: { enabled: true, duration: 180 },
  fen: game.positions[0].fen,
});

// ---------- motor ----------
const engine = new Engine({
  onInfo(job, info) {
    if (job.key !== game.key) return;
    const store = info.multipv === 2 ? game.seconds : game.evals;
    if (info.multipv <= 2 && info.depth >= (store[job.index]?.depth ?? 0)) {
      store[job.index] = info;
      if (info.multipv === 1) scheduleRender();
    }
  },
  onIdle: schedule,
  onReady() {
    $("engine-title").textContent = `Analysis · ${engine.label}`;
  },
  onError(error) {
    $("lines").replaceChildren(el("p", { class: "muted small", text: `The engine could not start on this device (${error.message}).` }));
  },
});

function schedule() {
  if (!engine.ready || engine.busy) return;
  const job = nextJob(game.positions.length, game.evals);
  if (!job) return;
  engine.analyze(game.positions[job.index].fen, job.depth, { index: job.index, key: game.key });
}

/** Nova posição: marca as posições terminadas e interrompe a análise se ela não for da última. */
function positionsChanged() {
  game.positions.forEach((pos, i) => {
    if (!game.evals[i]) {
      const known = terminalEval(pos.fen);
      if (known) game.evals[i] = known;
    }
  });
  if (engine.job && (engine.job.key !== game.key || engine.job.index !== game.positions.length - 1)) {
    engine.stop();
  } else {
    schedule();
  }
}

// ---------- desenho ----------
let renderQueued = false;
function scheduleRender() {
  if (renderQueued) return;
  renderQueued = true;
  requestAnimationFrame(() => {
    renderQueued = false;
    render();
  });
}

let lastBoardKey = null;
const shownIndex = () => (selected === null ? game.positions.length - 1 : selected);

function render() {
  const index = shownIndex();
  const pos = game.positions[index];
  const last = pos.uci ? [pos.uci.slice(0, 2), pos.uci.slice(2, 4)] : undefined;
  // Só mexe no tabuleiro quando a posição muda: a análise redesenha a página várias vezes por
  // segundo, e cada `set` reinicia a animação (que então nunca termina).
  const boardKey = `${pos.fen}|${pos.uci}|${game.botColor}`;
  if (boardKey !== lastBoardKey) {
    lastBoardKey = boardKey;
    const turn = pos.fen.split(" ")[1] === "w" ? "white" : "black";
    ground.set({ fen: pos.fen, lastMove: last, orientation: game.botColor, turnColor: turn, check: /[+#]$/.test(pos.san ?? "") });
  }
  renderPlayers();
  // A classificação vem antes da lista: a lista é refeita a cada desenho, já com as cores.
  const verdicts = classifyMoves(game.positions, game.evals, game.seconds, JUDGE_DEPTH);
  renderMoves(index, verdicts);
  renderAnalysis(index, verdicts);
  renderSummary();
}

function playerRow(color) {
  const p = game[color];
  if (!p) return [el("span", { class: "muted", text: color === game.botColor ? BOT : "waiting for a game" })];
  const label = color === "white" ? "White" : "Black";
  const who = el(
    "span",
    { class: "who" },
    el("span", { class: `piece-color ${color}`, title: label }),
    el("b", {}, playerLink(p.name, { class: "player-link" })),
    el("span", { class: "muted", text: `${p.rating ?? ""}` }),
    ...diffTag(color),
    el("span", { class: "muted", text: `· ${label}` }),
  );
  const clock = el("span", { class: "clock", id: `clock-${color}`, text: clockText(color) });
  return [who, clock];
}

// Variação de rating da partida terminada (vazia enquanto a partida corre ou se for casual).
function diffTag(color) {
  const diff = game.result?.ratingDiff?.[color];
  const text = formatDiff(diff);
  if (!text) return [];
  return [el("span", { class: `rating-diff ${diff > 0 ? "up" : diff < 0 ? "down" : ""}`, text })];
}

function renderPlayers() {
  const bottom = game.botColor;
  const top = bottom === "white" ? "black" : "white";
  $("player-top").replaceChildren(...playerRow(top));
  $("player-bottom").replaceChildren(...playerRow(bottom));
  tickClocks();
}

function clockText(color) {
  if (!game.clock) return "";
  let s = game.clock[color];
  if (typeof s !== "number" || !Number.isFinite(s)) return "";
  if (game.clock.running === color) s = Math.max(0, s - (Date.now() - game.clock.at) / 1000);
  const m = Math.floor(s / 60);
  const sec = Math.floor(s % 60);
  return `${m}:${String(sec).padStart(2, "0")}`;
}

function tickClocks() {
  for (const color of ["white", "black"]) {
    const node = document.getElementById(`clock-${color}`);
    if (!node) continue;
    node.textContent = clockText(color);
    node.classList.toggle("running", game.clock?.running === color);
  }
}
setInterval(tickClocks, 250);

function setEvalBar(score) {
  const white = 50 + 50 * winningChances(scoreToCp(score));
  $("evalbar").style.setProperty("--white", `${white.toFixed(1)}%`);
}

function renderAnalysis(index, verdicts) {
  const ev = game.evals[index];
  const second = game.seconds[index];
  const fen = game.positions[index].fen;
  if (ev) {
    // Posição terminada (mate ou empate) tem avaliação conhecida, não um número de motor.
    $("eval").textContent = ev.depth === Infinity ? (ev.score.cp === 0 ? "Draw" : "Checkmate") : formatScore(ev.score);
    setEvalBar(ev.score);
    const line = (info, cls) =>
      el("div", { class: cls }, el("b", { text: `${formatScore(info.score)}  ` }), uciLineToSan(fen, info.pv, 8).join(" "));
    if (ev.depth === Infinity) {
      $("lines").replaceChildren(el("div", { class: "muted", text: "The game is over." }));
    } else {
      const lines = [line(ev, "first")];
      // A 2ª linha só vale ao lado da 1ª se for da mesma profundidade (senão compara buscas diferentes).
      if (second && second.depth === ev.depth) lines.push(line(second, "second"));
      lines.push(el("div", { class: "muted small", text: `depth ${ev.depth}` }));
      $("lines").replaceChildren(...lines);
    }
  } else if (engine.ready) {
    $("eval").textContent = "…";
  }
  renderVerdict(index, verdicts);
  const acc = accuracies(game.positions, game.evals, JUDGE_DEPTH);
  if (acc.white != null || acc.black != null) {
    const f = (x) => (x == null ? "–" : x.toFixed(1));
    $("accuracy").textContent = `Accuracy ${game.white?.name ?? "White"} ${f(acc.white)} · ${game.black?.name ?? "Black"} ${f(acc.black)}`;
  } else {
    $("accuracy").textContent = "";
  }
}

function renderVerdict(index, verdicts) {
  const v = verdicts[index];
  const pos = game.positions[index];
  if (v && pos.san) {
    const info = LABELS[v.category];
    const text = v.category === "best" ? `${pos.san} was the engine's first choice` : `${pos.san}${v.best && v.best !== pos.san ? ` · best was ${v.best}` : ""}`;
    $("verdict").replaceChildren(el("span", { class: "badge", style: `background:${info.color}`, text: info.label }), el("span", { text }));
  } else {
    $("verdict").replaceChildren();
  }
}

function renderMoves(index, verdicts) {
  const list = $("moves");
  const items = [];
  for (let k = 1; k < game.positions.length; k += 2) {
    const no = Math.ceil(k / 2);
    const cell = (i) => {
      if (i >= game.positions.length) return el("span");
      const v = verdicts[i];
      const dot = el("span", { class: "cat", title: v ? LABELS[v.category].label : "" });
      if (v) dot.style.background = LABELS[v.category].color;
      return el("button", { class: `mv${i === index ? " current" : ""}`, type: "button", onclick: () => select(i) }, dot, game.positions[i].san);
    };
    items.push(el("li", {}, el("span", { class: "no", text: `${no}.` }), cell(k), cell(k + 1)));
  }
  list.replaceChildren(...items);
  if (selected === null) list.scrollTop = list.scrollHeight;
}

function select(i) {
  selected = i === game.positions.length - 1 && game.mode === "live" ? null : i;
  render();
}

function renderLegend() {
  const keys = ["brilliant", "great", "best", "inaccuracy", "mistake", "blunder"];
  $("legend").replaceChildren(...keys.map((k) => el("span", {}, el("i", { style: `background:${LABELS[k].color}` }), LABELS[k].label)));
}

// ---------- partida (ao vivo e revisão) ----------
function loadGame({ id, white, black, startFen, moves, mode, clock, speed }) {
  const key = `${mode}:${id}`;
  const positions = buildPositions(moves, startFen);
  const same = game.key === key && positions.length >= game.positions.length;
  if (!same) {
    game.evals = [];
    game.seconds = [];
    selected = null;
  }
  Object.assign(game, { key, id, white, black, startFen, positions, mode, clock, speed });
  game.botColor = white?.name?.toLowerCase() === BOT.toLowerCase() ? "white" : "black";
  positionsChanged();
  scheduleRender();
}

// Nome que leva ao perfil no Lichess, numa aba nova para não interromper a partida ao vivo.
function playerLink(name, attrs = {}) {
  const url = name && name !== "Anonymous" ? profileUrl(name) : null;
  return url ? el("a", { ...attrs, href: url, target: "_blank", rel: "noopener", text: name }) : el("span", { ...attrs, text: name ?? "" });
}

// Abertura e defesa/variação no topo da lista de lances.
function renderOpening() {
  const box = $("opening");
  const o = game.opening;
  box.hidden = !o;
  if (!o) return;
  box.replaceChildren(
    ...[o.eco ? el("span", { class: "eco", text: o.eco }) : null, el("b", { text: o.opening }), o.variation ? el("span", { text: ` · ${o.variation}` }) : null].filter(Boolean),
  );
}

function setOpening(data) {
  game.opening = splitOpening(data);
  renderOpening();
}

// Durante a partida o Lichess refina o nome da abertura a cada lance de livro.
async function refreshOpening() {
  if (game.mode !== "live") return;
  try {
    const current = await getJson(`${LICHESS}/api/user/${BOT}/current-game`);
    if (current.id === game.id && current.opening) setOpening(current.opening);
  } catch {
    // fica o nome anterior
  }
}

function playerOf(p) {
  return p ? { name: p.user?.name ?? p.name ?? "Anonymous", rating: p.rating ?? null, title: p.user?.title ?? null } : null;
}

async function loadHeadToHead(opponent) {
  if (!opponent) return;
  $("h2h-title").replaceChildren(`Head-to-head · ${BOT} vs `, playerLink(opponent, { class: "player-link" }));
  try {
    const r = await getJson(`/api/h2h?opponent=${encodeURIComponent(opponent)}`);
    $("h2h-wins").textContent = r.wins;
    $("h2h-draws").textContent = r.draws;
    $("h2h-losses").textContent = r.losses;
    $("h2h-count").textContent = `${r.games} games`;
    const word = { win: "W", draw: "D", loss: "L" };
    $("h2h-last").textContent = r.last.length ? `Last games: ${r.last.map((x) => word[x]).join(" ")}` : "First game between them";
    $("h2h-card").hidden = false;
  } catch {
    $("h2h-card").hidden = true;
  }
}

async function watchLive() {
  if (reviewing) return;
  let current;
  try {
    current = await getJson(`${LICHESS}/api/user/${BOT}/current-game`);
  } catch {
    return;
  }
  if (reviewing || current.status !== "started" || game.mode === "live") return;
  summaryOpen = false;
  game.result = null;
  setOpening(current.opening);
  game.tournament = current.arenaTour?.id ?? current.swissTour?.id ?? current.tournament ?? null;
  loadTournament(game.tournament);
  const white = playerOf(current.players.white);
  const black = playerOf(current.players.black);
  const opponent = white?.name?.toLowerCase() === BOT.toLowerCase() ? black?.name : white?.name;
  loadHeadToHead(opponent);
  const c = current.clock;
  $("game-line").textContent = `${game.tournament ? "Tournament game · " : ""}${current.rated ? "Rated" : "Casual"} ${current.speed}${c ? ` ${c.initial / 60}+${c.increment}` : ""} · `;
  $("game-line").append(el("a", { href: `${LICHESS}/${current.id}`, text: "open on Lichess" }));
  await streamGame(current.id, white, black);
}

async function streamGame(id, white, black) {
  streamAbort?.abort();
  const controller = new AbortController();
  streamAbort = controller;
  const events = [];
  const splitter = new NdjsonSplitter();
  game.mode = "live";
  try {
    const response = await fetch(`${LICHESS}/api/stream/game/${id}`, { signal: controller.signal, headers: { Accept: "application/x-ndjson" } });
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      if (reviewing) {
        controller.abort();
        return;
      }
      for (const ev of splitter.push(decoder.decode(value, { stream: true }))) {
        events.push(ev);
        if (!ev.fen) continue;
        const { startFen, moves, clock } = movesFromStream(events);
        const toMove = ev.fen.split(" ")[1] === "w" ? "white" : "black";
        loadGame({ id, white, black, startFen, moves, mode: "live", clock: clock ? { ...clock, at: Date.now(), running: moves.length >= 2 ? toMove : null } : null });
      }
    }
  } catch (error) {
    if (controller.signal.aborted) return;
  }
  if (streamAbort !== controller) return;
  // Fim da partida: o relógio para, o resumo abre e a lista de partidas e o histórico se atualizam.
  game.mode = "idle";
  if (game.clock) game.clock.running = null;
  try {
    let data = await getJson(`${LICHESS}/game/export/${id}?moves=false`);
    if (data.rated && !gameResult(data).ratingDiff) {
      // A variação de rating às vezes sai alguns segundos depois do fim.
      await new Promise((resolve) => setTimeout(resolve, 3000));
      data = await getJson(`${LICHESS}/game/export/${id}?moves=false`).catch(() => data);
    }
    game.result = gameResult(data);
    if (data.opening) setOpening(data.opening);
  } catch {
    game.result = { winner: null, status: "unknown", ratingDiff: null };
  }
  summaryOpen = true;
  addSummaryButton();
  scheduleRender();
  setStatus();
  loadGames();
}

async function review(id) {
  streamAbort?.abort();
  let data;
  try {
    data = await getJson(`${LICHESS}/game/export/${id}?moves=true&clocks=false&evals=false`);
  } catch {
    return;
  }
  const white = playerOf(data.players.white);
  const black = playerOf(data.players.black);
  const opponent = white?.name?.toLowerCase() === BOT.toLowerCase() ? black?.name : white?.name;
  loadHeadToHead(opponent);
  const outcome = !data.winner ? "draw" : data.players[data.winner]?.user?.name?.toLowerCase() === BOT.toLowerCase() ? `${BOT} won` : `${BOT} lost`;
  const score = !data.winner ? "½–½" : data.winner === "white" ? "1–0" : "0–1";
  $("game-line").replaceChildren(
    `Review · ${score} (${outcome}, ${data.status}) · ${data.rated ? "rated" : "casual"} ${data.speed} · `,
    el("a", { href: `${LICHESS}/${id}`, text: "open on Lichess" }),
    " · ",
    el("a", { href: "#", text: "back to live", onclick: (e) => { e.preventDefault(); location.hash = ""; } }),
  );
  game.result = gameResult(data);
  setOpening(data.opening);
  game.tournament = data.arenaTour?.id ?? data.swissTour?.id ?? data.tournament ?? null;
  loadTournament(game.tournament);
  summaryOpen = false;
  loadGame({ id, white, black, startFen: data.initialFen, moves: data.moves ? data.moves.split(" ") : [], mode: "review", clock: null });
  selected = null;
  addSummaryButton();
}

// ---------- campeonato ----------
let tourneyTimer = null;
async function loadTournament(id) {
  clearInterval(tourneyTimer);
  if (!id) {
    $("tourney").hidden = true;
    return;
  }
  const refresh = async () => {
    try {
      const t = await getJson(`${LICHESS}/api/tournament/${id}`);
      // A classificação é opcional: o Lichess limita exportações simultâneas por IP, e sem ela a
      // faixa continua com o nome e o tempo do torneio.
      // A primeira página da classificação já vem no resumo do torneio; a exportação completa só
      // é pedida se o bot não estiver nela.
      let rows = (t.standing?.players ?? []).map((p) => ({ username: p.name, rank: p.rank, score: p.score }));
      if (!rows.some((r) => r.username?.toLowerCase() === BOT.toLowerCase())) try {
        const response = await fetch(`${LICHESS}/api/tournament/${id}/results?nb=500`, { headers: { Accept: "application/x-ndjson" } });
        if (response.ok) {
          rows = (await response.text()).split("\n").filter((l) => l.trim()).map((l) => JSON.parse(l)).filter((r) => r.username);
        }
      } catch {
        rows = [];
      }
      const me = rows.find((r) => r.username.toLowerCase() === BOT.toLowerCase());
      const kind = t.system === "swiss" ? "Swiss" : t.teamBattle ? "Team battle" : "Arena";
      $("tourney-kind").textContent = `Tournament · ${kind}`;
      $("tourney-name").textContent = t.fullName ?? "Tournament";
      const secs = t.secondsToFinish;
      const h = Math.floor((secs || 0) / 3600);
      const m = Math.floor(((secs || 0) % 3600) / 60);
      const left = t.isFinished ? "finished" : secs ? `ends in ${h ? `${h}h ${String(m).padStart(2, "0")} min` : `${m} min`}` : "";
      const players = t.nbPlayers ?? rows.length;
      const standing = me ? `${BOT}: #${me.rank} of ${players} · ${me.score} pts` : `${players} players`;
      $("tourney-standing").textContent = [standing, left].filter(Boolean).join(" · ");
      $("tourney-link").href = `${LICHESS}/tournament/${id}`;
      $("tourney").hidden = false;
    } catch {
      $("tourney").hidden = true;
    }
  };
  await refresh();
  tourneyTimer = setInterval(refresh, 60000);
}

// ---------- resumo do fim da partida ----------
const STATUS_TEXT = {
  mate: "by checkmate",
  resign: "by resignation",
  outoftime: "on time",
  timeout: "on time",
  stalemate: "by stalemate",
  draw: "by agreement or rule",
  insufficientMaterialClaim: "by insufficient material",
};
const PHASE_EN = { opening: "Opening", middlegame: "Middlegame", endgame: "Endgame" };

function addSummaryButton() {
  if (document.getElementById("open-summary")) return;
  const button = el("button", { class: "btn", id: "open-summary", type: "button", text: "Summary" });
  button.addEventListener("click", () => {
    summaryOpen = true;
    render();
  });
  $("game-line").append(" · ", button);
}

function renderSummary() {
  const box = $("summary");
  if (!summaryOpen || !game.result) {
    box.hidden = true;
    return;
  }
  const s = gameSummary(game.positions, game.evals, game.seconds, JUDGE_DEPTH);
  const r = game.result;
  const title = !r.winner ? "Draw" : r.winner === game.botColor ? `${BOT} won` : `${BOT} lost`;
  const score = !r.winner ? "½–½" : r.winner === "white" ? "1–0" : "0–1";
  const f = (x) => (x == null ? "–" : x.toFixed(1));
  const accCell = (color) => {
    const p = game[color];
    const cls = color === game.botColor ? "acc me" : "acc";
    const diff = r.ratingDiff?.[color];
    const rating = p?.rating != null && typeof diff === "number" ? el("span", { class: "acc-rating" }, `${p.rating + diff} `, ...diffTag(color)) : null;
    return el("div", { class: cls }, ...[`${p?.name ?? color} · ${color === "white" ? "White" : "Black"}`, el("span", { class: "big", text: f(s[color].accuracy) }), "accuracy", rating].filter(Boolean));
  };
  const mine = s[game.botColor].counts;
  const pills = ["brilliant", "great", "best", "excellent"]
    .filter((k) => mine[k])
    .slice(0, 3)
    .map((k) => {
      const pill = el("span", { class: "pill", text: `${mine[k]} ${LABELS[k].label}` });
      pill.style.boxShadow = `inset 3px 0 0 ${LABELS[k].color}`;
      return pill;
    });
  const phases = el("div", { class: "phase-table" }, el("span", { class: "h" }), el("span", { class: "h", text: game.white?.name ?? "White" }), el("span", { class: "h", text: game.black?.name ?? "Black" }));
  for (const ph of ["opening", "middlegame", "endgame"]) {
    if (s.white.phases[ph] == null && s.black.phases[ph] == null) continue;
    phases.append(el("span", { text: PHASE_EN[ph] }), el("span", { class: "n", text: f(s.white.phases[ph]) }), el("span", { class: "n", text: f(s.black.phases[ph]) }));
  }
  const n = (side, ...keys) => keys.reduce((a, k) => a + (side.counts[k] || 0), 0);
  const errors = el(
    "div",
    { class: "errors" },
    el("span", { text: `Inaccuracies ${n(s.white, "inaccuracy")} / ${n(s.black, "inaccuracy")}` }),
    el("span", { text: `Mistakes ${n(s.white, "mistake", "miss")} / ${n(s.black, "mistake", "miss")}` }),
    el("span", { text: `Blunders ${n(s.white, "blunder")} / ${n(s.black, "blunder")}` }),
  );
  const close = el("button", { class: "btn", type: "button", "aria-label": "Close summary", text: "Close" });
  close.addEventListener("click", () => {
    summaryOpen = false;
    render();
  });
  const footer = s.analysed < s.total
    ? `Analysing: ${s.analysed} of ${s.total} moves done…`
    : game.mode === "review" ? "Use the moves list to step through the game." : "The next game appears here automatically.";
  // replaceChildren escreveria "null" como texto: os opcionais saem antes.
  const parts = [
    el(
      "div",
      { class: "summary-head" },
      el("div", {}, el("h2", { class: "summary-title", id: "summary-title", text: title }), el("span", { class: "summary-sub", text: `${STATUS_TEXT[r.status] ?? r.status} · ${score}${game.tournament ? " · tournament game" : ""}` })),
      close,
    ),
    el("div", { class: "acc-grid" }, accCell("white"), accCell("black")),
    pills.length ? el("div", { class: "pills" }, ...pills) : null,
    phases,
    errors,
    el("div", { class: "muted small", text: footer }),
  ].filter(Boolean);
  box.replaceChildren(...parts);
  box.hidden = false;
}

// ---------- topo, ranking, evolução, partidas ----------
async function setStatus() {
  try {
    const [s] = await getJson(`${LICHESS}/api/users/status?ids=${BOT}`);
    const status = $("status");
    status.classList.toggle("online", !!s.online);
    $("status-text").textContent = s.playing
      ? game.tournament && game.mode === "live" ? "playing in a tournament" : "playing now"
      : s.online ? "online · waiting for a game" : "offline";
  } catch {
    // mantém o texto anterior
  }
}

async function loadChips() {
  const chips = [];
  const chip = (label, value) => el("span", { class: "chip" }, `${label} `, el("b", { text: value }));
  try {
    const user = await getJson(`${LICHESS}/api/user/${BOT}`);
    const perf = (k) => user.perfs[k] && `${user.perfs[k].rating}${user.perfs[k].prov ? "?" : ""}`;
    if (perf("blitz")) chips.push(chip("Blitz", perf("blitz")));
    if (perf("rapid")) chips.push(chip("Rapid", perf("rapid")));
  } catch {
    // sem perfil, sem chips de rating
  }
  try {
    const ranking = await getJson("/api/ranking");
    if (ranking.me) chips.push(el("span", { class: "chip" }, "Bot ranking ", el("b", { text: `#${ranking.me.rank}` }), ` of ${ranking.total}`));
    renderRanking(ranking);
  } catch {
    $("ranking-meta").textContent = "ranking unavailable right now";
  }
  try {
    const strength = await getJson("data/strength.json");
    const latest = strength[strength.length - 1];
    chips.push(el("span", { class: "chip" }, "CCRL Blitz ", el("b", { text: `~${latest.elo}` }), " est."));
    renderChart(strength);
  } catch {
    // sem gráfico
  }
  $("chips").replaceChildren(...chips);
}

function renderRanking(ranking) {
  $("ranking-meta").textContent = `${ranking.total} ranked bots · ${ranking.live ? "live ratings" : "list"} · ${ranking.updated ?? "–"}`;
  const rows = [];
  ranking.rows.forEach((r, i) => {
    if (i > 0 && r.rank - ranking.rows[i - 1].rank > 1) rows.push(el("tr", { class: "gap" }, el("td", { colspan: "4" })));
    const delta = el("td", { class: `num ${r.delta.startsWith("+") ? "up" : r.delta.startsWith("-") ? "down" : "muted"}`, text: r.delta });
    rows.push(
      el(
        "tr",
        { class: r.me ? "me" : "" },
        el("td", { class: "num", text: `#${r.rank}` }),
        el("td", {}, playerLink(r.name)),
        el("td", { class: "num", text: String(r.rating) }),
        delta,
      ),
    );
  });
  $("ranking").replaceChildren(...rows);
}

function renderChart(points) {
  const NS = "http://www.w3.org/2000/svg";
  const W = 600, H = 190, L = 44, R = 12, T = 16, B = 26;
  const min = 1900, max = 3700;
  const x = (i) => L + (i * (W - L - R)) / (points.length - 1);
  const y = (e) => T + ((max - e) * (H - T - B)) / (max - min);
  const svg = document.createElementNS(NS, "svg");
  svg.setAttribute("viewBox", `0 0 ${W} ${H}`);
  svg.setAttribute("role", "img");
  svg.setAttribute("aria-label", `Estimated CCRL Blitz rating by version, from ${points[0].elo} to ${points[points.length - 1].elo}`);
  const add = (tag, attrs, text) => {
    const n = document.createElementNS(NS, tag);
    for (const [k, v] of Object.entries(attrs)) n.setAttribute(k, v);
    if (text) n.textContent = text;
    svg.append(n);
  };
  for (const e of [2000, 2500, 3000, 3500]) {
    add("line", { x1: L, x2: W - R, y1: y(e), y2: y(e), stroke: "#26241f" });
    add("text", { x: 4, y: y(e) + 4, fill: "#8c887f", "font-size": 11 }, String(e));
  }
  add("polyline", { fill: "none", stroke: "#7fb3e6", "stroke-width": 2.5, points: points.map((p, i) => `${x(i)},${y(p.elo)}`).join(" ") });
  points.forEach((p, i) => {
    add("circle", { cx: x(i), cy: y(p.elo), r: 3, fill: "#7fb3e6" }, null);
    const title = document.createElementNS(NS, "title");
    title.textContent = `${p.version} (${p.date}): ~${p.elo}`;
    svg.lastChild.append(title);
  });
  add("text", { x: L, y: H - 6, fill: "#8c887f", "font-size": 11 }, `${points[0].version} · ${points[0].date}`);
  add("text", { x: W - R, y: H - 6, fill: "#8c887f", "font-size": 11, "text-anchor": "end" }, `${points.at(-1).version} · ${points.at(-1).date}`);
  add("text", { x: x(points.length - 1) - 4, y: y(points.at(-1).elo) - 8, fill: "#f1eee8", "font-size": 12, "text-anchor": "end" }, `~${points.at(-1).elo}`);
  $("chart").replaceChildren(svg);
  $("chart").className = "chart";
}

async function loadGames() {
  try {
    const { games } = await getJson("/api/games");
    const word = { win: "Win", draw: "Draw", loss: "Loss" };
    $("games").replaceChildren(
      ...games.map((g) =>
        el(
          "li",
          {},
          el("span", { class: `res-${g.result}`, text: word[g.result] }),
          el("span", {}, "vs ", playerLink(g.opponent.name), ` (${g.opponent.rating ?? "?"}) · ${g.color}`, g.tournament ? el("span", { class: "tag-tourney", text: "Tournament" }) : null),
          el("span", { class: "muted", text: g.clock }),
          el("a", { href: `#review/${g.id}`, text: "Review" }),
        ),
      ),
    );
  } catch {
    $("games").replaceChildren(el("li", { class: "empty", text: "Recent games are unavailable right now." }));
  }
}

// ---------- rotas e início ----------
function route() {
  const m = location.hash.match(/^#review\/([A-Za-z0-9]{8})$/);
  if (m) {
    reviewing = m[1];
    streamAbort?.abort();
    review(m[1]);
  } else if (reviewing) {
    reviewing = null;
    game.mode = "idle";
    game.key = null;
    watchLive();
  }
}

window.addEventListener("hashchange", route);
document.addEventListener("keydown", (e) => {
  if (e.target.closest?.("input, textarea")) return;
  if (e.key === "ArrowLeft") select(Math.max(0, shownIndex() - 1));
  if (e.key === "ArrowRight") select(Math.min(game.positions.length - 1, shownIndex() + 1));
});

// Opção do motor completo: só aparece com isolamento entre origens e o build publicado.
if (self.crossOriginIsolated) {
  fetch("engine/stockfish-19.js", { method: "HEAD" })
    .then((r) => {
      if (r.ok) $("engine-opts").hidden = false;
    })
    .catch(() => {});
}
$("full-engine").addEventListener("change", (e) => {
  game.evals = [];
  game.seconds = [];
  engine.start(e.target.checked);
});

renderLegend();
scheduleRender();
engine.start(false);
setStatus();
loadChips();
loadGames();
route();
watchLive();
// Ranking e ratings do topo a cada minuto (a função da Vercel guarda o resultado por 60 s).
setInterval(loadChips, 60000);
setInterval(() => {
  setStatus();
  if (game.mode === "idle") watchLive();
  else refreshOpening();
}, 15000);

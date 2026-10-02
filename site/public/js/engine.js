// Stockfish 19 no navegador de quem visita, como Web Worker. Com isolamento entre origens (os
// cabeçalhos COOP/COEP do vercel.json) usa o build de várias threads; sem ele, o de uma thread.
import { parseInfo } from "./lib/analysis.js";

export class Engine {
  /**
   * `onInfo(job, info)` recebe cada linha `info` com pontuação (do lado das brancas);
   * `onIdle()` é chamado quando o motor termina uma análise e está livre.
   */
  constructor({ onInfo, onIdle, onReady, onError }) {
    this.onInfo = onInfo;
    this.onIdle = onIdle;
    this.onReady = onReady || (() => {});
    this.onError = onError || (() => {});
    this.worker = null;
    this.job = null;
    this.ready = false;
    this.stopping = false;
    this.name = "Stockfish 19";
  }

  /** `full`: o build completo (~95 MB), só com isolamento entre origens. */
  start(full = false) {
    this.worker?.terminate();
    this.ready = false;
    this.job = null;
    const threads = self.crossOriginIsolated ? Math.max(1, Math.min(4, (navigator.hardwareConcurrency || 2) - 1)) : 1;
    const file = !self.crossOriginIsolated
      ? "stockfish-19-lite-single.js"
      : full ? "stockfish-19.js" : "stockfish-19-lite.js";
    this.label = `${full && self.crossOriginIsolated ? "Stockfish 19" : "Stockfish 19 lite"}${threads > 1 ? ` · ${threads} threads` : ""}`;
    try {
      this.worker = new Worker(`engine/${file}`);
    } catch (error) {
      this.onError(error);
      return;
    }
    this.worker.onerror = (e) => this.onError(new Error(e.message || "falha ao carregar o motor"));
    this.worker.onmessage = (e) => this.line(String(e.data));
    this.send("uci");
    if (threads > 1) this.send(`setoption name Threads value ${threads}`);
    this.send("setoption name Hash value 64");
    this.send("setoption name MultiPV value 2");
    this.send("isready");
  }

  send(command) {
    this.worker?.postMessage(command);
  }

  line(text) {
    if (text === "readyok") {
      this.ready = true;
      this.onReady();
      this.onIdle();
    } else if (text.startsWith("info ") && this.job && !this.stopping) {
      const info = parseInfo(text, this.job.white);
      if (info) this.onInfo(this.job, info);
    } else if (text.startsWith("bestmove")) {
      this.job = null;
      this.stopping = false;
      this.onIdle();
    }
  }

  get busy() {
    return !!this.job;
  }

  /** Analisa `fen` até `depth`; `meta` volta junto em cada `onInfo`. */
  analyze(fen, depth, meta) {
    if (!this.ready || this.job) return false;
    this.job = { ...meta, fen, depth, white: fen.split(" ")[1] === "w" };
    this.send(`position fen ${fen}`);
    this.send(`go depth ${depth}`);
    return true;
  }

  /** Interrompe a análise atual (o `bestmove` que vem depois libera o motor). */
  stop() {
    if (this.job && !this.stopping) {
      this.stopping = true;
      this.send("stop");
    }
  }
}

// Copia para public/ o que vem do npm: o tabuleiro (chessground, GPL-3.0) e o Stockfish para
// navegador (stockfish 19, GPL-3.0). Nada disso vai para o git (ver .gitignore).
import { cpSync, mkdirSync } from "node:fs";

const copy = (from, to) => {
  cpSync(from, to);
  console.log(`${from} -> ${to}`);
};

mkdirSync("public/vendor/chessground", { recursive: true });
copy("node_modules/chessground/dist/chessground.min.js", "public/vendor/chessground/chessground.js");
for (const css of ["base", "brown", "cburnett"]) {
  copy(`node_modules/chessground/assets/chessground.${css}.css`, `public/vendor/chessground/${css}.css`);
}
copy("node_modules/chessground/LICENSE", "public/vendor/chessground/LICENSE");

mkdirSync("public/engine", { recursive: true });
// lite (várias threads e uma thread) sempre; o completo (~99 MB) só quando FULL_ENGINE=1.
const builds = ["stockfish-19-lite", "stockfish-19-lite-single"];
if (process.env.FULL_ENGINE === "1") builds.push("stockfish-19");
for (const name of builds) {
  copy(`node_modules/stockfish/bin/${name}.js`, `public/engine/${name}.js`);
  copy(`node_modules/stockfish/bin/${name}.wasm`, `public/engine/${name}.wasm`);
}
copy("node_modules/stockfish/Copying.txt", "public/engine/COPYING.txt");

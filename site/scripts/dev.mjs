// Servidor local que imita a Vercel: public/ com os cabeçalhos do vercel.json e /api/<nome> pelas
// funções de api/. Uso: `npm run build && node scripts/dev.mjs` → http://localhost:3000
// As funções de partidas precisam de LICHESS_TOKEN no ambiente (sem ele, respondem 502).
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize } from "node:path";

const PORT = Number(process.env.PORT || 3000);
const TYPES = { ".html": "text/html; charset=utf-8", ".js": "text/javascript", ".css": "text/css", ".json": "application/json", ".wasm": "application/wasm", ".txt": "text/plain" };
const config = JSON.parse(await readFile("vercel.json", "utf8"));

function headersFor(path) {
  const out = {};
  for (const rule of config.headers) {
    const pattern = new RegExp(`^${rule.source.replace("(.*)", ".*")}$`);
    if (pattern.test(path)) for (const h of rule.headers) out[h.key] = h.value;
  }
  return out;
}

createServer(async (req, res) => {
  const url = new URL(req.url, `http://localhost:${PORT}`);
  for (const [k, v] of Object.entries(headersFor(url.pathname))) res.setHeader(k, v);
  if (url.pathname.startsWith("/api/")) {
    const name = url.pathname.slice(5).replace(/[^a-z0-9-]/gi, "");
    try {
      const { default: handler } = await import(`../api/${name}.js`);
      req.query = Object.fromEntries(url.searchParams);
      res.status = (code) => ((res.statusCode = code), res);
      res.json = (body) => {
        res.setHeader("Content-Type", "application/json");
        res.end(JSON.stringify(body));
      };
      await handler(req, res);
    } catch (error) {
      res.statusCode = 404;
      res.end(String(error.message));
    }
    return;
  }
  const path = normalize(join("public", url.pathname === "/" ? "index.html" : url.pathname));
  if (!path.startsWith("public")) {
    res.statusCode = 403;
    res.end();
    return;
  }
  try {
    const body = await readFile(path);
    res.setHeader("Content-Type", TYPES[extname(path)] || "application/octet-stream");
    res.end(req.method === "HEAD" ? undefined : body);
  } catch {
    res.statusCode = 404;
    res.end("not found");
  }
}).listen(PORT, () => console.log(`http://localhost:${PORT}`));

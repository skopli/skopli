import { createReadStream, existsSync, statSync } from "node:fs";
import { createServer, type Server } from "node:http";
import { extname, join, normalize } from "node:path";

const types: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".md": "text/markdown; charset=utf-8",
  ".txt": "text/plain; charset=utf-8",
  ".xml": "application/xml",
  ".css": "text/css",
  ".js": "text/javascript",
  ".json": "application/json",
  ".svg": "image/svg+xml",
  ".woff2": "font/woff2",
  ".pf_meta": "application/octet-stream",
  ".pf_index": "application/octet-stream",
  ".pf_fragment": "application/octet-stream",
};

export function serveDist(root: string, port: number): Promise<Server> {
  const server = createServer((req, res) => {
    const pathname = decodeURIComponent(new URL(req.url ?? "/", "http://x").pathname);
    const safe = normalize(pathname).replace(/^(\.\.[/\\])+/, "");
    const candidates = [
      join(root, safe),
      join(root, `${safe}.html`),
      join(root, safe, "index.html"),
    ];
    const file = candidates.find((c) => existsSync(c) && statSync(c).isFile());
    const target = file ?? join(root, "skopli", "404.html");
    res.statusCode = file ? 200 : 404;
    res.setHeader("content-type", types[extname(target)] ?? "application/octet-stream");
    createReadStream(target).pipe(res);
  });
  return new Promise((resolve) => server.listen(port, () => resolve(server)));
}

if (process.argv[1]?.endsWith("serve-dist.ts")) {
  const port = Number(process.env.PORT ?? 4321);
  await serveDist(new URL("../dist", import.meta.url).pathname, port);
  console.log(`serving dist on http://localhost:${port}/skopli/`);
}

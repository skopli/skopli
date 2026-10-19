#!/usr/bin/env node
import { appendFileSync } from "node:fs";
import { createServer, type IncomingHttpHeaders, type Server } from "node:http";

type Json = Record<string, unknown>;
type Route = "chat" | "messages";

const obj = (v: unknown): Json | undefined =>
  v !== null && typeof v === "object" && !Array.isArray(v) ? (v as Json) : undefined;
const num = (v: unknown): number | undefined => (typeof v === "number" ? v : undefined);
const parse = (text: string): Json | undefined => {
  try {
    return obj(JSON.parse(text));
  } catch {
    return undefined;
  }
};

export const rewriteChat = (body: Json, timings = obj(body.timings)): Json => {
  const usage = obj(body.usage);
  const cacheN = num(timings?.cache_n);
  if (!usage || cacheN === undefined) return body;
  const details = { ...obj(usage.prompt_tokens_details), cached_tokens: cacheN };
  return { ...body, usage: { ...usage, prompt_tokens_details: details } };
};

export const rewriteMessageUsage = (usage: Json, cacheControl: boolean): Json => {
  const fresh = num(usage.input_tokens) ?? 0;
  return {
    ...usage,
    input_tokens: cacheControl ? 0 : fresh,
    cache_creation_input_tokens: cacheControl ? fresh : 0,
    cache_read_input_tokens: num(usage.cache_read_input_tokens) ?? 0,
  };
};

export const rewriteBody = (route: Route, text: string, cacheControl: boolean): string => {
  const body = parse(text);
  const usage = obj(body?.usage);
  if (!body || !usage) return text;
  if (route === "chat") return JSON.stringify(rewriteChat(body));
  return JSON.stringify({ ...body, usage: rewriteMessageUsage(usage, cacheControl) });
};

export const sseRewriter = (route: Route, cacheControl: boolean) => {
  let timings: Json | undefined;
  let input: Json = {};
  return (data: string): string => {
    const chunk = parse(data);
    if (!chunk) return data;
    if (route === "chat") {
      timings = obj(chunk.timings) ?? timings;
      return JSON.stringify(rewriteChat(chunk, timings));
    }
    const message = obj(chunk.message);
    const usage = obj(message?.usage);
    if (chunk.type === "message_start" && message && usage) {
      input = rewriteMessageUsage(usage, cacheControl);
      return JSON.stringify({ ...chunk, message: { ...message, usage: input } });
    }
    if (chunk.type === "message_delta" && obj(chunk.usage)) {
      return JSON.stringify({ ...chunk, usage: { ...input, ...obj(chunk.usage) } });
    }
    return data;
  };
};

const routes: Record<string, Route> = {
  "/v1/chat/completions": "chat",
  "/v1/messages": "messages",
};
const routeOf = (url: string): Route | undefined => routes[new URL(url, "http://gateway").pathname];

const dropped = new Set([
  "host",
  "connection",
  "content-length",
  "transfer-encoding",
  "content-encoding",
]);
const forwardable = (headers: IncomingHttpHeaders | Headers): Record<string, string> =>
  Object.fromEntries(
    [...(headers instanceof Headers ? headers : Object.entries(headers))]
      .filter(([k, v]) => !dropped.has(k.toLowerCase()) && typeof v === "string")
      .map(([k, v]) => [k, v as string]),
  );

export const createGateway = (upstream: URL, log?: string): Server =>
  createServer(async (req, res) => {
    const chunks: Buffer[] = [];
    for await (const chunk of req) chunks.push(chunk as Buffer);
    const body = Buffer.concat(chunks);
    const url = new URL(req.url ?? "/", upstream);
    let response: Response;
    try {
      const init: RequestInit = { method: req.method ?? "GET", headers: forwardable(req.headers) };
      response = await fetch(url, body.length ? { ...init, body } : init);
    } catch (error) {
      res.writeHead(502, { "content-type": "text/plain" });
      res.end(`gateway: upstream ${url} unreachable: ${String(error)}`);
      return;
    }
    const route = routeOf(req.url ?? "/");
    const cacheControl = body.includes('"cache_control"');
    const headers = forwardable(response.headers);
    const stream = response.headers.get("content-type")?.startsWith("text/event-stream") ?? false;
    const record = (usage: unknown) => {
      const line = {
        at: new Date().toISOString(),
        path: url.pathname,
        status: response.status,
        stream,
        cacheControl,
        usage,
      };
      if (log) appendFileSync(log, `${JSON.stringify(line)}\n`);
    };
    if (!route || !response.body) {
      res.writeHead(response.status, headers);
      for await (const chunk of response.body ?? []) res.write(chunk);
      res.end();
      return;
    }
    if (!stream) {
      const out = rewriteBody(route, await response.text(), cacheControl);
      record(parse(out)?.usage);
      res.writeHead(response.status, {
        ...headers,
        "content-length": String(Buffer.byteLength(out)),
      });
      res.end(out);
      return;
    }
    res.writeHead(response.status, headers);
    const rewrite = sseRewriter(route, cacheControl);
    let pending = "";
    let usage: unknown;
    const emit = (line: string) => {
      if (!line.startsWith("data: ")) {
        res.write(`${line}\n`);
        return;
      }
      const data = rewrite(line.slice(6));
      const seen = parse(data);
      usage = seen?.usage ?? obj(seen?.message)?.usage ?? usage;
      res.write(`data: ${data}\n`);
    };
    for await (const chunk of response.body) {
      pending += Buffer.from(chunk).toString("utf8");
      const lines = pending.split("\n");
      pending = lines.pop() ?? "";
      lines.forEach(emit);
    }
    if (pending) emit(pending);
    record(usage);
    res.end();
  });

if (import.meta.main) {
  const flag = (name: string, fallback: string) => {
    const i = process.argv.indexOf(name);
    return i === -1 ? fallback : (process.argv[i + 1] ?? fallback);
  };
  const port = Number(flag("--port", "8081"));
  const upstream = new URL(flag("--upstream", "http://127.0.0.1:8080"));
  const log = process.argv.includes("--log") ? flag("--log", "") : undefined;
  createGateway(upstream, log).listen(port, "127.0.0.1", () => {
    console.log(`gateway: http://127.0.0.1:${port} -> ${upstream.origin} pid ${process.pid}`);
  });
}

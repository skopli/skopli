#!/usr/bin/env node
import { appendFileSync } from "node:fs";
import { createServer, type IncomingHttpHeaders, type Server } from "node:http";

type Json = Record<string, unknown>;
type Route = "chat" | "messages" | "responses";

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

/** Anthropic marks cacheable prefixes with `cache_control` on system, tool and content blocks. */
export const carriesCacheControl = (body: Json | undefined): boolean => {
  const blocks = (v: unknown): Json[] =>
    Array.isArray(v) ? v.map(obj).filter((b): b is Json => b !== undefined) : [];
  const marked = (b: Json) => "cache_control" in b;
  return (
    [...blocks(body?.system), ...blocks(body?.tools)].some(marked) ||
    blocks(body?.messages).some((m) => blocks(m.content).some(marked))
  );
};

export const rewriteBody = (route: Route, text: string, cacheControl: boolean): string => {
  const body = parse(text);
  const usage = obj(body?.usage);
  if (!body || !usage || route === "responses") return text;
  if (route === "chat") return JSON.stringify(rewriteChat(body));
  return JSON.stringify({ ...body, usage: rewriteMessageUsage(usage, cacheControl) });
};

export const sseRewriter = (route: Route, cacheControl: boolean) => {
  let timings: Json | undefined;
  let input: Json = {};
  return (data: string): string => {
    const chunk = parse(data);
    if (!chunk || route === "responses") return data;
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

/** The usage block a chunk or body carries, wherever the three wires put it. */
export const usageOf = (body: Json | undefined): unknown =>
  body?.usage ?? obj(body?.message)?.usage ?? obj(body?.response)?.usage;

const routes: Record<string, Route> = {
  "/chat/completions": "chat",
  "/messages": "messages",
  "/responses": "responses",
};
export const routeOf = (url: string): Route | undefined => {
  const path = new URL(url, "http://gateway").pathname;
  return routes[path.startsWith("/v1/") ? path.slice(3) : path];
};
/** Harnesses whose base URL already ends in `/v1` post bare `/messages`; llama-server
 *  serves the routed endpoints under `/v1` only, so routed paths are forwarded there. */
export const upstreamUrl = (url: string, upstream: URL): URL => {
  const target = new URL(url, upstream);
  if (routeOf(url) && !target.pathname.startsWith("/v1/"))
    target.pathname = `/v1${target.pathname}`;
  return target;
};

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

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const refused = (error: unknown): boolean =>
  ["ECONNREFUSED", "ECONNRESET"].includes(String(obj(obj(error)?.cause)?.code));
const connect = async (url: URL, init: RequestInit, attempts = 3): Promise<Response> => {
  for (let i = 1; ; i++) {
    try {
      return await fetch(url, init);
    } catch (error) {
      if (i >= attempts || !refused(error)) throw error;
      await sleep(200 * i);
    }
  }
};

export type GatewayOptions = { log?: string | undefined; quiet?: boolean; json?: boolean };

export const createGateway = (upstream: URL, { log, quiet, json }: GatewayOptions = {}): Server =>
  createServer(async (req, res) => {
    const body = Buffer.concat(await req.toArray());
    const url = upstreamUrl(req.url ?? "/", upstream);
    // Progress goes to stderr; `json` makes each line machine-readable.
    const progress = (status: number, error?: string) => {
      if (quiet) return;
      const line = { method: req.method, path: url.pathname, status, ...(error ? { error } : {}) };
      console.error(
        json
          ? JSON.stringify(line)
          : `gateway: ${line.method} ${line.path} ${status}${error ? ` ${error}` : ""}`,
      );
    };
    let response: Response;
    try {
      const init: RequestInit = { method: req.method ?? "GET", headers: forwardable(req.headers) };
      response = await connect(url, body.length ? { ...init, body } : init);
    } catch (error) {
      res.writeHead(502, { "content-type": "text/plain" });
      res.end(`gateway: upstream ${url} unreachable: ${String(error)}`);
      progress(502, String(error));
      return;
    }
    const route = routeOf(req.url ?? "/");
    const cacheControl = route === "messages" && carriesCacheControl(parse(body.toString("utf8")));
    const headers = forwardable(response.headers);
    const stream = response.headers.get("content-type")?.startsWith("text/event-stream") ?? false;
    let aborted = false;
    res.on("close", () => {
      aborted = !res.writableFinished;
    });
    const record = (usage: unknown) => {
      const line = {
        at: new Date().toISOString(),
        path: url.pathname,
        status: response.status,
        stream,
        cacheControl,
        aborted,
        usage,
      };
      if (log) appendFileSync(log, `${JSON.stringify(line)}\n`);
      progress(response.status);
    };
    if (!route || !response.body) {
      res.writeHead(response.status, headers);
      for await (const chunk of response.body ?? []) res.write(chunk);
      res.end();
      return;
    }
    if (!stream) {
      const out = rewriteBody(route, await response.text(), cacheControl);
      res.writeHead(response.status, {
        ...headers,
        "content-length": String(Buffer.byteLength(out)),
      });
      res.end(out);
      record(usageOf(parse(out)));
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
      usage = usageOf(parse(data)) ?? usage;
      res.write(`data: ${data}\n`);
    };
    const decoder = new TextDecoder();
    for await (const chunk of response.body ?? []) {
      pending += decoder.decode(chunk, { stream: true });
      const lines = pending.split("\n");
      pending = lines.pop() ?? "";
      lines.forEach(emit);
    }
    pending += decoder.decode();
    if (pending) emit(pending);
    res.end();
    record(usage);
  });

const help = `usage: gateway.ts [--port <n>] [--upstream <url>] [--log <file>] [--quiet] [--json] [--dry-run]

Usage-rewriting proxy in front of a local llama-server. Forwards every request
unchanged and rewrites only the usage block of chat completions and messages
responses so a harness logs the cache fields its cloud provider would send.
Responses API calls pass through unchanged; their usage is logged as well.

  --port <n>        port to bind on 127.0.0.1 (default 8081)
  --upstream <url>  llama-server origin (default http://127.0.0.1:8080)
  --log <file>      append one JSON line per upstream response (route, status,
                    stream, cacheControl, aborted, usage as sent to the client)
  --quiet           no per-request lines on stderr
  --json            per-request stderr lines as JSON objects
  --dry-run         print the resolved configuration as JSON and exit
  --help            this text

exit codes: 0 ok, 1 could not listen, 2 usage error

examples:
  node gateway.ts --port 8081 --upstream http://127.0.0.1:8080 --log usage.jsonl
  OPENAI_BASE_URL=http://127.0.0.1:8081/v1 OPENAI_API_KEY=sk-dummy <harness>
  ANTHROPIC_BASE_URL=http://127.0.0.1:8081 ANTHROPIC_API_KEY=sk-ant-dummy <harness>`;

if (import.meta.main) {
  const argv = process.argv.slice(2);
  const known = new Set([
    "--port",
    "--upstream",
    "--log",
    "--quiet",
    "--json",
    "--dry-run",
    "--help",
  ]);
  const flag = (name: string) => {
    const i = argv.indexOf(name);
    return i === -1 ? undefined : argv[i + 1];
  };
  const unknown = argv.filter(
    (a, i) => a.startsWith("--") && !known.has(a) && argv[i - 1] !== "--log",
  );
  if (argv.includes("--help")) {
    console.log(help);
    process.exit(0);
  }
  if (unknown.length) {
    console.error(`gateway: unknown option ${unknown.join(" ")}\n\n${help}`);
    process.exit(2);
  }
  const config = {
    port: Number(flag("--port") ?? "8081"),
    upstream: flag("--upstream") ?? "http://127.0.0.1:8080",
    log: flag("--log"),
    quiet: argv.includes("--quiet"),
    json: argv.includes("--json"),
  };
  if (!Number.isInteger(config.port) || config.port <= 0 || !URL.canParse(config.upstream)) {
    console.error(`gateway: bad --port or --upstream\n\n${help}`);
    process.exit(2);
  }
  if (argv.includes("--dry-run")) {
    console.log(JSON.stringify(config, null, 2));
    process.exit(0);
  }
  const server = createGateway(new URL(config.upstream), config);
  server.on("error", (error) => {
    console.error(`gateway: cannot listen on 127.0.0.1:${config.port}: ${error.message}`);
    process.exit(1);
  });
  server.listen(config.port, "127.0.0.1", () => {
    console.log(
      JSON.stringify({
        gateway: `http://127.0.0.1:${config.port}`,
        upstream: config.upstream,
        pid: process.pid,
      }),
    );
  });
}

import { mkdtempSync, readFileSync } from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, describe, expect, it } from "vitest";
import {
  createGateway,
  rewriteBody,
  rewriteChat,
  rewriteMessageUsage,
  routeOf,
  upstreamUrl,
  sseRewriter,
  usageOf,
} from "./gateway.ts";

const chat = {
  choices: [],
  usage: {
    prompt_tokens: 44,
    completion_tokens: 4,
    total_tokens: 48,
    prompt_tokens_details: { cached_tokens: 0 },
  },
  timings: { cache_n: 26, prompt_n: 18, predicted_n: 4 },
};
const message = { cache_read_input_tokens: 26, input_tokens: 20, output_tokens: 6 };

describe("chat completions", () => {
  it("sets cached_tokens from timings.cache_n", () => {
    const usage = rewriteChat(chat).usage as { prompt_tokens_details: { cached_tokens: number } };
    expect(usage.prompt_tokens_details.cached_tokens).toBe(26);
  });

  it("leaves a body without timings alone", () => {
    const { timings: _, ...bare } = chat;
    expect(rewriteChat(bare)).toEqual(bare);
    expect(rewriteBody("chat", "not json", false)).toBe("not json");
  });

  it("carries timings from an earlier chunk onto the usage chunk of a stream", () => {
    const rewrite = sseRewriter("chat", false);
    expect(rewrite("[DONE]")).toBe("[DONE]");
    rewrite(JSON.stringify({ choices: [{ delta: {} }], timings: chat.timings }));
    const usage = JSON.parse(rewrite(JSON.stringify({ choices: [], usage: chat.usage }))).usage;
    expect(usage.prompt_tokens_details).toEqual({ cached_tokens: 26 });
  });
});

describe("messages", () => {
  it("keeps server-reported cache reads and no creation without cache_control", () => {
    expect(rewriteMessageUsage(message, false)).toEqual({
      input_tokens: 20,
      cache_creation_input_tokens: 0,
      cache_read_input_tokens: 26,
      output_tokens: 6,
    });
  });

  it("books newly processed tokens as cache creation when the request carried cache_control", () => {
    expect(rewriteMessageUsage(message, true)).toEqual({
      input_tokens: 0,
      cache_creation_input_tokens: 20,
      cache_read_input_tokens: 26,
      output_tokens: 6,
    });
    expect(
      JSON.parse(rewriteBody("messages", JSON.stringify({ usage: message }), true)).usage
        .input_tokens,
    ).toBe(0);
  });

  it("rewrites message_start and makes message_delta usage cumulative", () => {
    const rewrite = sseRewriter("messages", true);
    const start = JSON.parse(
      rewrite(
        JSON.stringify({
          type: "message_start",
          message: { usage: { ...message, output_tokens: 0 } },
        }),
      ),
    );
    expect(start.message.usage).toEqual({
      input_tokens: 0,
      cache_creation_input_tokens: 20,
      cache_read_input_tokens: 26,
      output_tokens: 0,
    });
    const text = rewrite(JSON.stringify({ type: "content_block_delta", delta: { text: "hi" } }));
    expect(JSON.parse(text).type).toBe("content_block_delta");
    const delta = JSON.parse(
      rewrite(JSON.stringify({ type: "message_delta", usage: { output_tokens: 6 } })),
    );
    expect(delta.usage).toEqual({
      input_tokens: 0,
      cache_creation_input_tokens: 20,
      cache_read_input_tokens: 26,
      output_tokens: 6,
    });
  });
});

describe("routes", () => {
  it("matches the three wires with or without the /v1 prefix", () => {
    expect(routeOf("/v1/chat/completions")).toBe("chat");
    expect(routeOf("/chat/completions?x=1")).toBe("chat");
    expect(routeOf("/v1/messages")).toBe("messages");
    expect(routeOf("/v1/responses")).toBe("responses");
    expect(routeOf("/v1/models")).toBeUndefined();
  });
  it("forwards routed paths under /v1 and leaves the rest alone", () => {
    const up = new URL("http://127.0.0.1:8080");
    expect(upstreamUrl("/messages", up).pathname).toBe("/v1/messages");
    expect(upstreamUrl("/v1/messages", up).pathname).toBe("/v1/messages");
    expect(upstreamUrl("/chat/completions?x=1", up).href).toBe(
      "http://127.0.0.1:8080/v1/chat/completions?x=1",
    );
    expect(upstreamUrl("/health", up).pathname).toBe("/health");
  });

  it("finds usage in a body, a message_start, or a response.completed event", () => {
    expect(usageOf({ usage: { a: 1 } })).toEqual({ a: 1 });
    expect(usageOf({ type: "message_start", message: { usage: { b: 2 } } })).toEqual({ b: 2 });
    expect(usageOf({ type: "response.completed", response: { usage: { c: 3 } } })).toEqual({
      c: 3,
    });
    expect(usageOf({ type: "response.output_text.delta" })).toBeUndefined();
  });

  it("never rewrites responses bodies or streams", () => {
    const text = JSON.stringify({
      usage: { input_tokens: 44, input_tokens_details: { cached_tokens: 43 } },
    });
    expect(rewriteBody("responses", text, false)).toBe(text);
    expect(sseRewriter("responses", false)(text)).toBe(text);
  });
});

describe("proxy", () => {
  const log = join(mkdtempSync(join(tmpdir(), "gateway-")), "usage.jsonl");
  const sse = [
    "event: message_start\n",
    `data: ${JSON.stringify({ type: "message_start", message: { usage: { ...message, output_tokens: 0 } } })}\n\n`,
    `event: message_delta\ndata: ${JSON.stringify({ type: "message_delta", usage: { output_tokens: 6 } })}\n\n`,
  ].join("");
  const upstream = createServer((req, res) => {
    const chunks: Buffer[] = [];
    req.on("data", (c: Buffer) => chunks.push(c));
    req.on("end", () => {
      if (req.url === "/v1/responses") {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(Buffer.concat(chunks));
        return;
      }
      res.writeHead(200, { "content-type": "text/event-stream" });
      const cut = sse.indexOf("input_tokens");
      res.write(sse.slice(0, cut));
      setTimeout(() => res.end(sse.slice(cut)), 10);
    });
  });
  const listen = (server: Server) =>
    new Promise<string>((resolve) =>
      server.listen(0, "127.0.0.1", () =>
        resolve(`http://127.0.0.1:${(server.address() as AddressInfo).port}`),
      ),
    );
  const ready = listen(upstream).then((origin) =>
    listen(createGateway(new URL(origin), { log, quiet: true })),
  );
  afterAll(() => upstream.close());
  const logged = () =>
    readFileSync(log, "utf8")
      .trim()
      .split("\n")
      .map((l) => JSON.parse(l) as Record<string, unknown>);

  it("rewrites a stream whose data line is split across chunks", async () => {
    const res = await fetch(`${await ready}/v1/messages`, {
      method: "POST",
      body: JSON.stringify({
        messages: [
          {
            role: "user",
            content: [{ type: "text", text: "x", cache_control: { type: "ephemeral" } }],
          },
        ],
      }),
    });
    expect(res.headers.get("content-type")).toBe("text/event-stream");
    const lines = (await res.text()).split("\n").filter((l) => l.startsWith("data: "));
    expect(JSON.parse(lines[0]!.slice(6)).message.usage.cache_creation_input_tokens).toBe(20);
    expect(JSON.parse(lines[1]!.slice(6)).usage).toEqual({
      input_tokens: 0,
      cache_creation_input_tokens: 20,
      cache_read_input_tokens: 26,
      output_tokens: 6,
    });
  });

  it("forwards responses untouched and still logs their usage", async () => {
    const usage = { input_tokens: 44, input_tokens_details: { cached_tokens: 43 } };
    const body = JSON.stringify({ usage });
    const res = await fetch(`${await ready}/v1/responses`, { method: "POST", body });
    expect(await res.text()).toBe(body);
    const line = logged().find((l) => l.path === "/v1/responses");
    expect(line).toMatchObject({ status: 200, stream: false, aborted: false, usage });
  });

  it("serves a bare /messages from the upstream /v1/messages", async () => {
    const res = await fetch(`${await ready}/messages`, { method: "POST", body: "{}" });
    expect(res.status).toBe(200);
    expect((await res.text()).includes("message_start")).toBe(true);
  });
  it("retries a refused upstream connection before answering 502", async () => {
    const late = createServer((_req, res) => res.end("{}"));
    const port = (upstream.address() as AddressInfo).port + 1000;
    const gateway = await listen(
      createGateway(new URL(`http://127.0.0.1:${port}`), { quiet: true }),
    );
    setTimeout(() => late.listen(port, "127.0.0.1"), 100);
    const res = await fetch(`${gateway}/v1/models`);
    late.close();
    expect(res.status).toBe(200);
  });
  it("logs the rewritten usage of a stream", async () => {
    const line = logged().find((l) => l.path === "/v1/messages");
    expect(line).toMatchObject({
      stream: true,
      cacheControl: true,
      usage: { cache_creation_input_tokens: 20, cache_read_input_tokens: 26, output_tokens: 6 },
    });
  });
});

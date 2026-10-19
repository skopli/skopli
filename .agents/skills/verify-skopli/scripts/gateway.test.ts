import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { afterAll, describe, expect, it } from "vitest";
import {
  createGateway,
  rewriteBody,
  rewriteChat,
  rewriteMessageUsage,
  sseRewriter,
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

describe("proxy", () => {
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
  const ready = listen(upstream).then((origin) => listen(createGateway(new URL(origin))));
  afterAll(() => upstream.close());

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

  it("forwards other routes untouched", async () => {
    const body = JSON.stringify({
      usage: { input_tokens: 44, input_tokens_details: { cached_tokens: 43 } },
    });
    const res = await fetch(`${await ready}/v1/responses`, { method: "POST", body });
    expect(await res.text()).toBe(body);
  });
});

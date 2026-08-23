import { expect, it } from "vitest";

it("scaffold placeholder", async () => {
  const mod = await import("../src/index.ts");
  expect(mod).toBeDefined();
});

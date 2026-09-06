import { mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

type CacheFile = {
  fetchedAt: string;
  payload: unknown;
};

export type CachedPayload = {
  fetchedAt: string;
  payload: unknown;
  stale: boolean;
};

export function loadCached(
  cacheDir: string,
  name: string,
  ttlMs: number,
  now: number = Date.now(),
): CachedPayload | null {
  let raw: string;
  try {
    raw = readFileSync(join(cacheDir, name), "utf8");
  } catch {
    return null;
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return null;
  }
  if (typeof parsed !== "object" || parsed === null) return null;
  const file = parsed as Partial<CacheFile>;
  if (typeof file.fetchedAt !== "string" || file.payload === undefined) return null;
  const age = now - Date.parse(file.fetchedAt);
  if (!Number.isFinite(age)) return null;
  return { fetchedAt: file.fetchedAt, payload: file.payload, stale: age > ttlMs };
}

export function storeCached(
  cacheDir: string,
  name: string,
  payload: unknown,
  now: number = Date.now(),
): string {
  const fetchedAt = new Date(now).toISOString();
  const file: CacheFile = { fetchedAt, payload };
  mkdirSync(cacheDir, { recursive: true });
  // write-then-rename so a concurrent reader never sees a torn file
  const tmp = join(cacheDir, `${name}.${process.pid}.${Math.random().toString(36).slice(2)}.tmp`);
  writeFileSync(tmp, JSON.stringify(file));
  try {
    renameSync(tmp, join(cacheDir, name));
  } catch (error) {
    rmSync(tmp, { force: true });
    throw error;
  }
  return fetchedAt;
}

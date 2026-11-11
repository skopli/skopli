import { readFileSync } from "node:fs";
import { join } from "node:path";

// Every place a release version is written by hand, in the order CONTRIBUTING.md lists them.
const locations = [
  ["Cargo.toml", "workspace.package.version", /^\[workspace\.package\][^[]*?^version = "([^"]*)"/m],
  [
    "Cargo.toml",
    "workspace.dependencies.skopli-core",
    /^skopli-core = \{[^}]*\bversion = "([^"]*)"/m,
  ],
  [
    "Cargo.toml",
    "workspace.dependencies.skopli-wire",
    /^skopli-wire = \{[^}]*\bversion = "([^"]*)"/m,
  ],
  ["package.json", "version", /^ {2}"version": "([^"]*)"/m],
  ["crates/skopli-py/pyproject.toml", "project.version", /^\[project\][^[]*?^version = "([^"]*)"/m],
  ["crates/skopli-ruby/lib/skopli/version.rb", "Skopli::VERSION", /^\s*VERSION = "([^"]*)"/m],
  ["sdks/java/build.gradle.kts", "version", /^version = "([^"]*)"/m],
  ["sdks/csharp/Skopli/Skopli.csproj", "<Version>", /<Version>([^<]*)<\/Version>/],
];

export const readVersions = (root) =>
  locations.map(([file, field, pattern]) => ({
    where: `${file} ${field}`,
    version: readFileSync(join(root, file), "utf8").match(pattern)?.[1],
  }));

export const versionErrors = (versions, tag) => {
  const missing = versions.filter((v) => v.version === undefined);
  if (missing.length) return missing.map((v) => `${v.where}: no version found`);
  const expected = tag === undefined ? versions[0].version : tag.replace(/^v/, "");
  const source = tag === undefined ? versions[0].where : `tag ${tag}`;
  return versions
    .filter((v) => v.version !== expected)
    .map((v) => `${v.where}: ${v.version}, but ${source} is ${expected}`);
};

const main = () => {
  const args = process.argv.slice(2);
  if (args.length > 1 || args.some((a) => a.startsWith("-"))) {
    console.error("usage: node scripts/check-versions.mjs [vX.Y.Z]");
    process.exit(2);
  }
  const versions = readVersions(join(import.meta.dirname, ".."));
  const errors = versionErrors(versions, args[0] || undefined);
  if (errors.length) {
    console.error(errors.join("\n"));
    process.exit(1);
  }
  console.log(`${versions.length} manifest versions are ${versions[0].version}`);
};

if (process.argv[1] === import.meta.filename) main();

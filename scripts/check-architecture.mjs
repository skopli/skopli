import { readFileSync } from "node:fs";
import { join } from "node:path";
import { parseSync } from "oxc-parser";

const identifier = (node, name) => node?.type === "Identifier" && node.name === name;

const zeroArgumentCall = (node, name) =>
  node?.type === "CallExpression" && identifier(node.callee, name) && node.arguments.length === 0;

const jsonStringifyOf = (node, name) =>
  node?.type === "CallExpression" &&
  node.callee?.type === "MemberExpression" &&
  identifier(node.callee.object, "JSON") &&
  identifier(node.callee.property, "stringify") &&
  node.arguments.length === 1 &&
  identifier(node.arguments[0], name);

const declaration = (statement) =>
  statement.type === "ExportNamedDeclaration" && statement.declaration
    ? statement.declaration
    : statement;

export const rollupFindings = (text) => {
  const parsed = parseSync("rollup.ts", text, { sourceType: "module" });
  if (parsed.errors.length)
    return parsed.errors.map((error) => `rollup facade does not parse: ${error.message}`);
  const statements = parsed.program.body;
  const nativeImports = new Map();
  for (const statement of statements) {
    if (statement.type !== "ImportDeclaration" || statement.source.value !== "./native.ts")
      continue;
    for (const specifier of statement.specifiers) {
      if (specifier.type === "ImportSpecifier")
        nativeImports.set(specifier.imported.name, specifier.local.name);
    }
  }

  const findings = [];
  const nativeName = nativeImports.get("native");
  const fromWireName = nativeImports.get("fromWire");
  if (!nativeName || !fromWireName)
    findings.push("src/rollup.ts must import native and fromWire from ./native.ts");

  const functions = statements
    .map(declaration)
    .filter((node) => node.type === "FunctionDeclaration");
  const rollupFunction = functions.find((node) => node.id?.name === "rollup");
  if (functions.length !== 1 || !rollupFunction) {
    findings.push("src/rollup.ts must contain rollup as its only function implementation");
  } else {
    const eventParam =
      rollupFunction.params[0]?.type === "Identifier" ? rollupFunction.params[0].name : "events";
    const optionsParam =
      rollupFunction.params[1]?.type === "Identifier" ? rollupFunction.params[1].name : "options";
    const body = rollupFunction.body?.body ?? [];
    const argument =
      body.length === 1 && body[0].type === "ReturnStatement" ? body[0].argument : undefined;
    const valid =
      argument?.type === "CallExpression" &&
      identifier(argument.callee, fromWireName) &&
      argument.arguments.length === 1 &&
      argument.arguments[0]?.type === "CallExpression" &&
      argument.arguments[0].callee?.type === "MemberExpression" &&
      identifier(argument.arguments[0].callee.property, "rollup") &&
      zeroArgumentCall(argument.arguments[0].callee.object, nativeName) &&
      argument.arguments[0].arguments.length === 2 &&
      jsonStringifyOf(argument.arguments[0].arguments[0], eventParam) &&
      jsonStringifyOf(argument.arguments[0].arguments[1], optionsParam);
    if (!valid) {
      findings.push(
        "rollup must be only return fromWire(native().rollup(JSON.stringify(events), JSON.stringify(options)))",
      );
    }
  }

  for (const statement of statements) {
    const node = declaration(statement);
    if (
      node.type === "ImportDeclaration" ||
      node.type === "TSTypeAliasDeclaration" ||
      node.type === "FunctionDeclaration"
    )
      continue;
    if (node.type === "VariableDeclaration") {
      const declarators = node.declarations;
      if (
        declarators.length === 1 &&
        declarators[0].id.type === "Identifier" &&
        declarators[0].id.name === "DEFAULT_BLOCK_MS" &&
        declarators[0].init?.type === "Literal" &&
        typeof declarators[0].init.value === "number"
      ) {
        continue;
      }
    }
    findings.push(`unexpected runtime statement in thin rollup facade: ${node.type}`);
  }
  return [...new Set(findings)];
};

const main = () => {
  const path = join(import.meta.dirname, "..", "src", "rollup.ts");
  const findings = rollupFindings(readFileSync(path, "utf8"));
  if (findings.length) {
    console.error(findings.join("\n"));
    console.error(
      "architecture: keep rollup math in crates/skopli-core and make the TypeScript facade a thin wire adapter",
    );
    process.exit(1);
  }
  console.log("architecture rules hold: TypeScript rollup delegates to the Rust core");
};

if (process.argv[1] === import.meta.filename) main();

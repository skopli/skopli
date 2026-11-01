// Shared command-line parsing for the verification scripts: --help exits 0 with the help text,
// an unknown option or a bad invocation exits 2 with the message and the help text on stderr.
export const parseArgs = (help, valued = [], flags = []) => {
  const argv = process.argv.slice(2);
  if (argv.includes("--help")) {
    console.log(help);
    process.exit(0);
  }
  const usage = (message) => {
    console.error(`${message}\n\n${help}`);
    process.exit(2);
  };
  const known = new Set([...valued, ...flags, "--dry-run", "--quiet", "--help"]);
  const isValue = (i) => valued.includes(argv[i - 1]);
  const unknown = argv.filter((a, i) => a.startsWith("--") && !known.has(a) && !isValue(i));
  if (unknown.length) usage(`unknown option ${unknown.join(" ")}`);
  const many = (name) =>
    argv.flatMap((a, i) => (a === name && i + 1 < argv.length ? [argv[i + 1]] : []));
  const flag = (name) => argv.includes(name);
  return {
    positional: argv.filter((a, i) => !a.startsWith("--") && !isValue(i)),
    many,
    one: (name, fallback) => many(name)[0] ?? fallback,
    flag,
    dryRun: flag("--dry-run"),
    log: flag("--quiet") ? () => {} : (line) => console.error(line),
    usage,
  };
};

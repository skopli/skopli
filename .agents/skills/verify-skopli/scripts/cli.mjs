// Shared command-line parsing for the verification scripts: --help exits 0 with the help text,
// an unknown option or a bad invocation exits 2 with the message and the help text on stderr.
export const parseArgs = (help, valued = []) => {
  const argv = process.argv.slice(2);
  if (argv.includes("--help")) {
    console.log(help);
    process.exit(0);
  }
  const usage = (message) => {
    console.error(`${message}\n\n${help}`);
    process.exit(2);
  };
  const known = new Set([...valued, "--dry-run", "--quiet", "--help"]);
  const isValue = (i) => valued.includes(argv[i - 1]);
  const unknown = argv.filter((a, i) => a.startsWith("--") && !known.has(a) && !isValue(i));
  if (unknown.length) usage(`unknown option ${unknown.join(" ")}`);
  const many = (flag) =>
    argv.flatMap((a, i) => (a === flag && i + 1 < argv.length ? [argv[i + 1]] : []));
  return {
    positional: argv.filter((a, i) => !a.startsWith("--") && !isValue(i)),
    many,
    one: (flag, fallback) => many(flag)[0] ?? fallback,
    dryRun: argv.includes("--dry-run"),
    log: argv.includes("--quiet") ? () => {} : (line) => console.error(line),
    usage,
  };
};

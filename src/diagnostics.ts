import type { Diagnostic, DiagnosticSeverity, Harness } from "./types.ts";

export type DiagnosticSink = {
  add(
    severity: DiagnosticSeverity,
    message: string,
    extra?: { harness?: Harness; path?: string },
  ): void;
  warn(message: string, extra?: { harness?: Harness; path?: string }): void;
  list(): Diagnostic[];
};

export function createDiagnostics(): DiagnosticSink {
  const diagnostics: Diagnostic[] = [];
  const add: DiagnosticSink["add"] = (severity, message, extra) => {
    const diagnostic: Diagnostic = { severity, message };
    if (extra?.harness !== undefined) diagnostic.harness = extra.harness;
    if (extra?.path !== undefined) diagnostic.path = extra.path;
    diagnostics.push(diagnostic);
  };
  return {
    add,
    warn(message, extra) {
      add("warning", message, extra);
    },
    list(): Diagnostic[] {
      return diagnostics;
    },
  };
}

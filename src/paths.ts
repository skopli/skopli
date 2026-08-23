import { homedir } from "node:os";

export type PathResolverOptions = {
  home?: string;
  // when provided, this map is the entire environment; process.env is not
  // consulted, so tests and relocated setups get full isolation
  env?: Readonly<Record<string, string | undefined>>;
  // the working directory project-local harnesses (e.g. trae) resolve from;
  // explicit value wins for test isolation, otherwise the process cwd
  cwd?: string;
};

export type PathResolver = {
  home(): string;
  env(name: string): string | undefined;
  cwd(): string | undefined;
};

// The effective home directory for a set of path options: an explicit `home`
// wins, otherwise the process home. Single source of truth shared by
// `createPathResolver` and the read facade's addon envelope.
export function effectiveHome(options: PathResolverOptions = {}): string {
  return options.home ?? homedir();
}

// The effective environment source for a set of path options: an explicit `env`
// map is the ENTIRE environment (process.env is not consulted); otherwise the
// process environment. Empty-string values are treated as unset by callers.
export function effectiveEnv(
  options: PathResolverOptions = {},
): Readonly<Record<string, string | undefined>> {
  return options.env ?? process.env;
}

// The effective working directory for a set of path options: an explicit `cwd`
// wins, otherwise the process working directory. Used by project-local readers
// that resolve roots relative to the working directory rather than home.
export function effectiveCwd(options: PathResolverOptions = {}): string {
  return options.cwd ?? process.cwd();
}

export function createPathResolver(options: PathResolverOptions = {}): PathResolver {
  return {
    home(): string {
      return effectiveHome(options);
    },
    env(name: string): string | undefined {
      const value = effectiveEnv(options)[name];
      return value === "" ? undefined : value;
    },
    cwd(): string | undefined {
      return effectiveCwd(options);
    },
  };
}

export const defaultPathResolver: PathResolver = createPathResolver();

# Publishing operations

This guide records the external account configuration and credential maintenance that cannot be inferred from the repository.
It contains no secret values and is safe to keep under version control.

Every publish job in `.github/workflows/release.yml` is gated on the repo variable `PUBLISH_ENABLED == 'true'`, and a registry accepts a publish only once its trusted publisher below is configured.
The GitHub release is always created as a draft, and the registries publish only after a human publishes it (see [Release flow](#release-flow)).

The permanent home is a GitHub organization (`github.com/skopli`, falling back to `skopli-dev`), created before the first release.
The Go module path and every OIDC trusted-publisher configuration bake in the owner/repo identity, so the owner must be frozen first.
Replace `skopli/skopli` below if the slug differs.
A free npm organization `skopli` is also created to reserve the `@skopli/` scope defensively; published packages stay unscoped.
The canonical domain is **skopli.com** (skopli.dev held as a backup/redirect), which fixes the Maven namespace as `com.skopli`.

## Release flow

`release.yml` has two event graphs, and no job in one waits for a job in the other.

1. Pushing a `vX.Y.Z` tag starts `version-check`, which fails the run before any build unless every manifest version is `X.Y.Z` (`scripts/check-versions.mjs`).
2. The build legs build every artifact.
   Each leg whose target runs on its own runner also loads what it built: the napi addon, a wheel, the C ABI library, the npm tarball, the gem, the NuGet package and the jar.
3. `draft-release` attaches the npm tarball, the wheels and sdist, the C ABI archives, the gem and the NuGet package to a draft release, with a `SHA256SUMS` file that covers them.
   `attach-java-jar` then adds the jars and their `.sha256` files, so the jar, which no registry receives yet, never holds up the draft.
4. A maintainer reviews the draft and publishes it.
5. Publishing fires `release: published`, which starts the five publish jobs when `PUBLISH_ENABLED` is `'true'`.
   The npm, PyPI, RubyGems and NuGet jobs download their files from that release and check them against `SHA256SUMS` before pushing, so each registry receives the reviewed bytes.
   The crates.io job publishes from the tag's source, which has no release asset to check.

Every publish job skips a version its registry already has, so a failed publish job can be re-run on its own.
Once a release is published, `draft-release` fails instead of touching it, so a later run of the same tag cannot replace the reviewed files.
The build run and the publish run of one tag share a concurrency group, so publishing a draft while its tag's run is still attaching the jar queues the publish run behind it.
The group holds one waiting run, so a newer run of the same tag cancels a waiting publish run.
Nothing fires `release: published` a second time: when the publish run was cancelled, or ran while `PUBLISH_ENABLED` was unset, re-run it from the Actions tab.

`workflow_dispatch` with `dry_run: true` (the default) runs `version-check` and the build legs, and touches no release.
With `dry_run: false` it also drafts the release; dispatch it on the tag (`gh workflow run release.yml --ref vX.Y.Z -f dry_run=false`) so the draft and the version check use that tag.
A dispatch never publishes to a registry.

## Current channels

| Channel                 | Credential                                      | External state                                                                                                                                  |
| ----------------------- | ----------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| GitHub Releases         | `GITHUB_TOKEN`                                  | Workflow is ready (draft-only); the first release is pending.                                                                                   |
| npm                     | Trusted publishing (OIDC), no stored secret     | Nothing published. `skopli` was free at check (2026-08-23). Complete the local [npm bootstrap](#npm) before the first release.                  |
| PyPI                    | Trusted publishing (OIDC), no stored secret     | Nothing published; `skopli` free at check. Complete [PyPI](#pypi).                                                                              |
| crates.io               | Trusted publishing (OIDC), no stored secret     | No crates published; `skopli*` names free at check. Complete [crates.io](#cratesio).                                                            |
| RubyGems                | Trusted publishing (OIDC), no stored secret     | Nothing published; `skopli` free at check. Complete [RubyGems](#rubygems).                                                                      |
| NuGet                   | Trusted publishing (OIDC), no stored secret     | Nothing published; `Skopli` free at check. Complete [NuGet](#nuget).                                                                            |
| Maven Central           | Portal token + GPG signing key (no OIDC exists) | No `com.skopli` namespace claimed; no publish job exists yet. Complete [Maven Central](#maven-central).                                         |
| C ABI (GitHub Releases) | `GITHUB_TOKEN`                                  | One `skopli-capi-<version>-<target>` archive per target (header, shared and static library) attaches to the draft release. See [C ABI](#c-abi). |
| Go modules              | None (the tag is the release)                   | Module path `github.com/skopli/skopli` becomes permanent at first `go get`. See [Go](#go-modules).                                              |
| Swift Package Manager   | None (consumers pin the repo tag)               | No registry interaction. See [Swift](#swift-package-manager).                                                                                   |

## Trusted publishing (OIDC)

npm, PyPI, crates.io, RubyGems, and NuGet all support GitHub Actions OIDC trusted publishing as of 2026-08: the workflow exchanges its short-lived OIDC identity for a single-use publish credential, so no long-lived token is stored.
Each requires `id-token: write` on the publish job and a trusted-publisher configuration on the registry naming this owner, repository, and workflow filename (`release.yml`).
Maven Central is the one channel that cannot use OIDC.

All five gated publish jobs in `release.yml` are OIDC-native: `id-token: write` plus the registry's official exchange (npm CLI, pinned to 12.0.1, `pypa/gh-action-pypi-publish`, `rust-lang/crates-io-auth-action`, `rubygems/configure-rubygems-credentials`, `NuGet/login`).
No classic registry token is referenced anywhere; the only remaining stored secrets are Maven Central's (which has no OIDC).

### npm

The package is the unscoped `skopli`.
It is one package that carries the napi loader and the prebuilt binary of every `napi-prebuild` target under `native/`; there are no per-platform packages.
The `npm-package` job packs it once with `pnpm pack`, the draft release carries that `skopli-<version>.tgz`, and `publish-npm` publishes exactly that file.

npm trusted publishing requires the package to already exist, so bootstrap it **locally, before the first release**, using the central bootstrap script.
It publishes a `0.0.0` placeholder under the `bootstrap` dist-tag from your own logged-in npm session, configures the trusted publisher, locks publishing to 2FA, and verifies the result - no `NPM_TOKEN` is ever stored in the repository:

```sh
gh api repos/jishnuteegala/.github/contents/scripts/npm-oidc-bootstrap.sh --jq .content | base64 -d > npm-oidc-bootstrap.sh
bash npm-oidc-bootstrap.sh all --repo skopli/skopli --workflow release.yml skopli
```

The `publish` and `lock` phases prompt for web-based EOTP, so run them interactively.
An agent may run the `trust` and `verify` phases without an OTP.

The bootstrap configures the npm trusted publisher:

| Field                | Value         |
| -------------------- | ------------- |
| Organization or user | `skopli`      |
| Repository           | `skopli`      |
| Workflow filename    | `release.yml` |
| Environment          | Leave empty   |
| Allowed actions      | `npm publish` |

The job runs `npm publish skopli-<version>.tgz --provenance --access public` with `id-token: write`; the package page should show the "Built and signed on GitHub Actions" provenance badge.
After configuring, lock token publishing:

```sh
npm access set mfa=publish skopli
```

With the bootstrap done there is no interim token: the publish job references no `NPM_TOKEN`; never create one.
Verify provenance after every release:

```sh
version=0.1.0
npm view "skopli@$version" --json dist.attestations \
  | node -e 'const a=JSON.parse(require("fs").readFileSync(0,"utf8"));process.exit(a&&a.provenance?0:1)' \
  && echo 'skopli: provenance OK' || echo 'skopli: NO PROVENANCE'
```

### PyPI

The `publish-pypi` job uploads the wheels and sdist attached to the published release with `pypa/gh-action-pypi-publish` + `id-token: write`; no API token is stored.
Configure a "pending" trusted publisher on pypi.org before the first release (pending publishers allow the very first upload):

| Field             | Value         |
| ----------------- | ------------- |
| PyPI project name | `skopli`      |
| Owner             | `skopli`      |
| Repository        | `skopli`      |
| Workflow filename | `release.yml` |
| Environment       | Leave empty   |

### crates.io

The `publish-crates` job publishes `skopli-core`, then `skopli-wire`, then `skopli-capi`, in dependency order.
The three manifests allow publishing and share the workspace version; the `skopli-node`, `skopli-py`, `skopli-ruby` binding crates stay `publish = false`.
The job checks out the published release's tag, checks that the tag matches the crate version, skips any crate already on crates.io, and runs `cargo publish -p <crate>`; cargo waits for each crate to reach the index before verifying the next.

crates.io only lets a crate that already exists get a trusted publisher, so the first version of each crate is published by hand with the interim token below: `cargo publish -p skopli-core`, then `-p skopli-wire`, then `-p skopli-capi`.
Then add the trusted publisher (owner `skopli`, repository `skopli`, workflow `release.yml`) on each of the three crates, revoke the token, and let the job mint its own with `rust-lang/crates-io-auth-action` + `id-token: write` from the next release on.

Interim token path: a crates.io token created at
<https://crates.io/settings/tokens/new> with:

| Field      | Value                                   |
| ---------- | --------------------------------------- |
| Name       | `skopli-release`                        |
| Expiration | 90 days (add a rotation reminder)       |
| Scopes     | `publish-new` and `publish-update` only |
| Crates     | The pattern `skopli*`, not Unrestricted |

Do not grant `change-owners`, `yank`, or `trusted-publishing`.

### RubyGems

The gem is `skopli` (built from `crates/skopli-ruby`, rb-sys/magnus).
The `publish-rubygems` job downloads `skopli.gem` from the published release, runs `rubygems/configure-rubygems-credentials` + `id-token: write`, then `gem push` unless RubyGems already has the version; no API key is stored.
RubyGems trusted publishing supports a "pending" publisher for a gem name that has never been pushed - configure it before the first release with owner `skopli`, repository `skopli`, workflow `release.yml`.

Decide before v1: source gem (compiles on `gem install`, current setup) vs precompiled native gems (rb-sys cross-gem matrix, multiplies the build matrix).

### NuGet

The package is `Skopli` (`sdks/csharp/Skopli`).
The `publish-nuget` job downloads the `.nupkg` from the published release, runs `NuGet/login` (OIDC exchanged for a one-hour temporary API key), then `dotnet nuget push --skip-duplicate`; no API key is stored.
Configure the trusted-publishing policy on nuget.org for owner `skopli`, repository `skopli`, workflow `release.yml`.
A successful push still goes through nuget.org validation before listing.

### Maven Central

The only channel that requires stored secrets, and the only one with no publish job yet (the workflow attaches the jar to the GitHub release but publishes it nowhere).
Before the first Java release:

1. Register on <https://central.sonatype.com> and claim the `com.skopli`
   namespace by DNS TXT verification of skopli.com.
2. Generate a portal token (username/password pair) on the Central Portal
   account page; store as `MAVEN_CENTRAL_USERNAME` / `MAVEN_CENTRAL_PASSWORD`.
3. Generate a release-only GPG keypair, publish the public key to
   `keyserver.ubuntu.com`, and store the private key and passphrase as
   `MAVEN_GPG_PRIVATE_KEY` / `MAVEN_GPG_PASSPHRASE`:

```sh
gpg --quick-generate-key "skopli release <release@skopli.com>" ed25519 sign 2y
gpg --keyserver keyserver.ubuntu.com --send-keys <KEYID>
gpg --export-secret-keys --armor <KEYID> | gh secret set MAVEN_GPG_PRIVATE_KEY --repo skopli/skopli
```

4. Publish via the Central Publishing plugin (or an equivalent portal-API
   action) with artifacts signed by that key.

Isolate all four secrets in a protected `release` GitHub Actions environment.

### C ABI

The C/C++ surface (`skopli-capi`: cdylib + staticlib + cbindgen `skopli.h`) has no package registry.
It is distributed as GitHub release assets: the `capi-artifacts` matrix builds six targets and `scripts/stage-capi.sh` packs each into `skopli-capi-<version>-<target>.tar.gz` (`.zip` for Windows) with `skopli.h`, the shared library and the static library inside, so the assets keep distinct names on the release.
`cargo publish` of `skopli-capi` gives source-level consumers `cargo`/build-from-source access.
Optional later channels if demand appears - vcpkg or Conan submissions, and Homebrew/system packages - each a separate manifest repo and credential; do not set these up before a user asks.

### Go modules

There is no publish step: `go install github.com/skopli/skopli/sdks/go@vX.Y.Z` resolves straight from the git tag.
Requirements:

- Tags must be exact `vX.Y.Z` semver.
- The module path is permanent; a later owner change is a breaking module-path migration with no redirect.
  This is why the org must exist before the first tagged release.
- A major version ≥ 2 forces a `/v2` module-path suffix.

Add a post-release smoke job that runs `go build` against the fresh tag.

### Swift Package Manager

No registry: consumers add the repo URL and pin the tag in `Package.swift`.
Nothing to configure beyond pushing the tag.
Add a post-release smoke job that runs `swift build` against the fresh tag.

## Setting secrets

Only Maven Central, the one channel without OIDC, needs stored secrets.
Let `gh` prompt so values do not enter shell history:

```sh
gh secret set MAVEN_CENTRAL_USERNAME --repo skopli/skopli
gh secret set MAVEN_CENTRAL_PASSWORD --repo skopli/skopli
gh secret set MAVEN_GPG_PASSPHRASE --repo skopli/skopli
```

Enable publishing only by setting the repo variable after every gate above is satisfied:

```sh
gh variable set PUBLISH_ENABLED --repo skopli/skopli --body true
```

List secret names and update times with `gh secret list --repo skopli/skopli`.
GitHub never reveals stored secret values; a recent update time proves only that a value was stored, not that it is valid.

## Monitoring

After every release:

1. Confirm the tag's **Release** run completed, the draft release was
   reviewed and published by a human, and the publish run that followed completed.
2. Confirm `skopli` exists on npm at the new version with provenance linked to
   this repository.
3. Confirm the PyPI wheels (manylinux/musllinux/windows/macos matrix) and sdist
   appear at the new version.
4. Confirm `skopli-core`, `skopli-wire`, and `skopli-capi` appear on crates.io
   in that order.
5. Confirm the `skopli` gem version on rubygems.org.
6. Confirm the `Skopli` package passed nuget.org validation and is listed.
7. Confirm `com.skopli:skopli` is released (not stuck in a staging deployment)
   on Maven Central, once the Java channel exists.
8. Confirm `go install github.com/skopli/skopli/...@vX.Y.Z` and a
   `swift build` against the tag both succeed.

Use **Actions -> Release -> Run workflow** with `dry_run: true` to rebuild all artifacts without touching any release.
It must never create a new version.

## Rotation and incident response

Review credentials quarterly and after maintainer, repository, or account changes.
Also monitor provider expiry emails and failed publishing jobs.

| Credential                                           | Normal maintenance                                     | Rotation procedure                                                                                                                                                                                                     |
| ---------------------------------------------------- | ------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| npm/PyPI/crates.io/RubyGems/NuGet trusted publishers | Audit each registry's publisher config quarterly       | No secret rotation; update the publisher immediately if the owner, repository, or workflow filename changes. If OIDC breaks, temporarily allow a short-lived scoped token only to recover, then restore and revoke it. |
| Interim registry tokens (any still stored)           | Check expiry and scope quarterly                       | Create replacement, update secret, prove it on the next publication, revoke old token. Delete each one permanently once its channel is on OIDC.                                                                        |
| `MAVEN_CENTRAL_USERNAME` / `MAVEN_CENTRAL_PASSWORD`  | Check portal account and namespace quarterly           | Regenerate the portal token, update both secrets, prove on the next deployment.                                                                                                                                        |
| `MAVEN_GPG_PRIVATE_KEY` / `MAVEN_GPG_PASSPHRASE`     | Check key expiry (2y) and keyserver presence quarterly | Generate a new keypair before expiry, publish the public key, update secrets, sign the next release with it.                                                                                                           |

If a credential may be exposed, revoke or remove it at the provider first, rotate it, inspect workflow and provider audit logs, and rerun only after the new credential is installed.
Deleting a GitHub secret alone does not revoke the credential at its provider.

## Repository controls

- Keep default workflow permissions read-only; `release.yml` grants
  `contents: write` only to the draft-release and attach-java-jar jobs.
- Prevent Actions from approving pull requests.
- Require maintainer approval before workflows from external forks run.
- Require CI checks on `main`.
- Protect immutable `v*` tags.
- Keep all third-party actions pinned to full commit SHAs (already done in
  `ci.yml` and `release.yml`) and enforce it in repository settings.
- Keep PR CI on `pull_request` with read-only permissions and no secrets.
- Enable org-wide required 2FA and add a second org owner early.
- Keep publishing credentials in GitHub Actions secrets (Maven's in a protected
  `release` environment), never in this file.

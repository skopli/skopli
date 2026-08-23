# Publishing operations

This guide records the external account configuration and credential maintenance
that cannot be inferred from the repository. It contains no secret values and is
safe to keep under version control.

This repository is currently LOCAL-ONLY. Every publish job in
`.github/workflows/release.yml` is gated on the repo variable
`PUBLISH_ENABLED == 'true'` and on a per-registry secret that does not exist, so
nothing can publish until a real repository is deliberately configured. The
GitHub release is always created as a draft; a human publishes it.

The permanent home is a GitHub organization (`github.com/skopli`, falling back
to `skopli-dev`), created before the first release. The Go module path and
every OIDC trusted-publisher configuration bake in the owner/repo identity, so
the owner must be frozen first. Replace `skopli/skopli` below if the slug
differs. A free npm organization `skopli` is also created to reserve the
`@skopli/` scope defensively; published packages stay unscoped. The canonical
domain is **skopli.com** (skopli.dev held as a backup/redirect), which fixes
the Maven namespace as `com.skopli`.

## Current channels

| Channel                 | Credential                                      | External state                                                                                                                                                             |
| ----------------------- | ----------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| GitHub Releases         | `GITHUB_TOKEN`                                  | Workflow is ready (draft-only); the first release is pending.                                                                                                              |
| npm                     | Trusted publishing (OIDC), no stored secret     | Nothing published. `skopli` and the `skopli-<platform>` prebuild names were free at check (2026-08-23). Complete the local [npm bootstrap](#npm) before the first release. |
| PyPI                    | Trusted publishing (OIDC), no stored secret     | Nothing published; `skopli` free at check. Complete [PyPI](#pypi).                                                                                                         |
| crates.io               | Trusted publishing (OIDC), no stored secret     | No crates published; `skopli*` names free at check. Complete [crates.io](#cratesio).                                                                                       |
| RubyGems                | Trusted publishing (OIDC), no stored secret     | Nothing published; `skopli` free at check. Complete [RubyGems](#rubygems).                                                                                                 |
| NuGet                   | Trusted publishing (OIDC), no stored secret     | Nothing published; `Skopli` free at check. Complete [NuGet](#nuget).                                                                                                       |
| Maven Central           | Portal token + GPG signing key (no OIDC exists) | No `com.skopli` namespace claimed; no publish job exists yet. Complete [Maven Central](#maven-central).                                                                    |
| C ABI (GitHub Releases) | `GITHUB_TOKEN`                                  | Per-target cdylib/staticlib + `skopli.h` already attach to the draft release. See [C ABI](#c-abi).                                                                         |
| Go modules              | None (the tag is the release)                   | Module path `github.com/skopli/skopli` becomes permanent at first `go get`. See [Go](#go-modules).                                                                         |
| Swift Package Manager   | None (consumers pin the repo tag)               | No registry interaction. See [Swift](#swift-package-manager).                                                                                                              |

## Trusted publishing (OIDC)

npm, PyPI, crates.io, RubyGems, and NuGet all support GitHub Actions OIDC
trusted publishing as of 2026-08: the workflow exchanges its short-lived OIDC
identity for a single-use publish credential, so no long-lived token is stored.
Each requires `id-token: write` on the publish job and a trusted-publisher
configuration on the registry naming this owner, repository, and workflow
filename (`release.yml`). Maven Central is the one channel that cannot use OIDC.

All five gated publish jobs in `release.yml` are OIDC-native: `id-token: write`
plus the registry's official exchange (npm CLI >= 11.5.1, `pypa/gh-action-pypi-publish`,
`rust-lang/crates-io-auth-action`, `rubygems/configure-rubygems-credentials`,
`NuGet/login`). No classic registry token is referenced anywhere; the only
remaining stored secrets are Maven Central's (which has no OIDC).

### npm

The main package is the unscoped `skopli`; napi-rs manages the
`skopli-<platform>` prebuild packages as `optionalDependencies` (unscoped, so
trusted publishing must be configured for every one of them).

npm trusted publishing requires the package to already exist, so bootstrap each
package **locally, before the first release**, using the central bootstrap
script. It publishes a `0.0.0` placeholder under the `bootstrap` dist-tag from
your own logged-in npm session, configures the trusted publisher, locks
publishing to 2FA, and verifies the result — no `NPM_TOKEN` is ever stored in
the repository:

```sh
gh api repos/jishnuteegala/.github/contents/scripts/npm-oidc-bootstrap.sh --jq .content | base64 -d > npm-oidc-bootstrap.sh
bash npm-oidc-bootstrap.sh all --repo skopli/skopli --workflow release.yml \
  skopli skopli-darwin-arm64 skopli-darwin-x64 \
  skopli-linux-x64-gnu skopli-linux-arm64-gnu \
  skopli-linux-x64-musl skopli-linux-arm64-musl \
  skopli-linux-arm-gnueabihf \
  skopli-win32-x64-msvc skopli-win32-arm64-msvc
```

The `publish` and `lock` phases prompt for web-based EOTP, so run them
interactively. An agent may run the `trust` and `verify` phases without an OTP.
Keep the platform list in sync with the shipped prebuild matrix.

The bootstrap configures one npm trusted publisher per package:

| Field                | Value         |
| -------------------- | ------------- |
| Organization or user | `skopli`      |
| Repository           | `skopli`      |
| Workflow filename    | `release.yml` |
| Environment          | Leave empty   |
| Allowed actions      | `npm publish` |

Publish with `npm publish --provenance --access public` and `id-token: write`;
the package pages should show the "Built and signed on GitHub Actions"
provenance badge. After configuring, lock token publishing:

```sh
npm access set mfa=publish skopli
```

With the bootstrap done there is no interim token: delete the `NPM_TOKEN`
references from the publish job when converting it to OIDC, and never create
one. Verify provenance after every release:

```sh
version=0.1.0
npm view "skopli@$version" --json dist.attestations \
  | node -e 'const a=JSON.parse(require("fs").readFileSync(0,"utf8"));process.exit(a&&a.provenance?0:1)' \
  && echo 'skopli: provenance OK' || echo 'skopli: NO PROVENANCE'
```

### PyPI

Replace the `MATURIN_PYPI_TOKEN` upload with
`pypa/gh-action-pypi-publish@release/v1` + `id-token: write`, or keep maturin
upload with a project-scoped API token. Configure a "pending" trusted publisher
on pypi.org before the first release (pending publishers allow the very first
upload):

| Field             | Value         |
| ----------------- | ------------- |
| PyPI project name | `skopli`      |
| Owner             | `skopli`      |
| Repository        | `skopli`      |
| Workflow filename | `release.yml` |
| Environment       | Leave empty   |

### crates.io

Publish order is dependency order with index-propagation waits:
`skopli-core` → `skopli-wire` → `skopli-capi` (the `skopli-node`, `skopli-py`,
`skopli-ruby` binding crates are not published to crates.io). Trusted
publishing is configured per crate on crates.io; use
`rust-lang/crates-io-auth-action` + `id-token: write` to mint a temporary
token, then `cargo publish` each crate.

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

The gem is `skopli` (built from `crates/skopli-ruby`, rb-sys/magnus). Replace
`gem push` + `RUBYGEMS_API_KEY` with `rubygems/release-gem@v1` +
`id-token: write`. RubyGems trusted publishing supports a "pending" publisher
for a gem name that has never been pushed — configure it before the first
release with owner `skopli`, repository `skopli`, workflow `release.yml`.

Decide before v1: source gem (compiles on `gem install`, current setup) vs
precompiled native gems (rb-sys cross-gem matrix, multiplies the build matrix).

### NuGet

The package is `Skopli` (`sdks/csharp/Skopli`). Replace the `NUGET_API_KEY`
push with `NuGet/login@v1` (OIDC → one-hour temporary API key) followed by
`dotnet nuget push`. Configure the trusted-publishing policy on nuget.org for
owner `skopli`, repository `skopli`, workflow `release.yml`. A successful push
still goes through nuget.org validation before listing.

### Maven Central

The only channel that requires stored secrets, and the only one with no publish
job yet (the workflow builds `java-jar` but does not publish it). Before the
first Java release:

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

The C/C++ surface (`skopli-capi`: cdylib + staticlib + cbindgen `skopli.h`) has
no package registry. It is distributed as GitHub release assets: the
`capi-artifacts` matrix already builds six targets and attaches
`capi-<target>` archives to the draft release, and `cargo publish` of
`skopli-capi` gives source-level consumers `cargo`/build-from-source access.
Optional later channels if demand appears — vcpkg or Conan submissions, and
Homebrew/system packages — each a separate manifest repo and credential; do not
set these up before a user asks.

### Go modules

There is no publish step: `go install github.com/skopli/skopli/sdks/go@vX.Y.Z`
resolves straight from the git tag. Requirements:

- Tags must be exact `vX.Y.Z` semver.
- The module path is permanent; a later owner change is a breaking module-path
  migration with no redirect. This is why the org must exist before the first
  tagged release.
- A major version ≥ 2 forces a `/v2` module-path suffix.

Add a post-release smoke job that runs `go build` against the fresh tag.

### Swift Package Manager

No registry: consumers add the repo URL and pin the tag in `Package.swift`.
Nothing to configure beyond pushing the tag. Add a post-release smoke job that
runs `swift build` against the fresh tag.

## Setting secrets

Only needed for channels not yet migrated to OIDC. Let `gh` prompt so values do
not enter shell history:

```sh
gh secret set PYPI_TOKEN --repo skopli/skopli
gh secret set CARGO_REGISTRY_TOKEN --repo skopli/skopli
gh secret set RUBYGEMS_API_KEY --repo skopli/skopli
gh secret set NUGET_API_KEY --repo skopli/skopli
gh secret set MAVEN_CENTRAL_USERNAME --repo skopli/skopli
gh secret set MAVEN_CENTRAL_PASSWORD --repo skopli/skopli
gh secret set MAVEN_GPG_PASSPHRASE --repo skopli/skopli
```

Enable publishing only by setting the repo variable after every gate above is
satisfied:

```sh
gh variable set PUBLISH_ENABLED --repo skopli/skopli --body true
```

List secret names and update times with `gh secret list --repo skopli/skopli`.
GitHub never reveals stored secret values; a recent update time proves only
that a value was stored, not that it is valid.

## Monitoring

After every release:

1. Confirm the **Release** workflow completed and the draft release was
   reviewed and published by a human.
2. Confirm `skopli` and every `skopli-<platform>` package exist on npm at the
   new version with provenance linked to this repository.
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

Use **Actions -> Release -> Run workflow** with `dry_run: true` to rebuild all
artifacts without touching any release. It must never create a new version.

## Rotation and incident response

Review credentials quarterly and after maintainer, repository, or account
changes. Also monitor provider expiry emails and failed publishing jobs.

| Credential                                           | Normal maintenance                                     | Rotation procedure                                                                                                                                                                                                     |
| ---------------------------------------------------- | ------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| npm/PyPI/crates.io/RubyGems/NuGet trusted publishers | Audit each registry's publisher config quarterly       | No secret rotation; update the publisher immediately if the owner, repository, or workflow filename changes. If OIDC breaks, temporarily allow a short-lived scoped token only to recover, then restore and revoke it. |
| Interim registry tokens (any still stored)           | Check expiry and scope quarterly                       | Create replacement, update secret, prove it on the next publication, revoke old token. Delete each one permanently once its channel is on OIDC.                                                                        |
| `MAVEN_CENTRAL_USERNAME` / `MAVEN_CENTRAL_PASSWORD`  | Check portal account and namespace quarterly           | Regenerate the portal token, update both secrets, prove on the next deployment.                                                                                                                                        |
| `MAVEN_GPG_PRIVATE_KEY` / `MAVEN_GPG_PASSPHRASE`     | Check key expiry (2y) and keyserver presence quarterly | Generate a new keypair before expiry, publish the public key, update secrets, sign the next release with it.                                                                                                           |

If a credential may be exposed, revoke or remove it at the provider first,
rotate it, inspect workflow and provider audit logs, and rerun only after the
new credential is installed. Deleting a GitHub secret alone does not revoke the
credential at its provider.

## Repository controls

- Keep default workflow permissions read-only; `release.yml` grants
  `contents: write` only to the draft-release job.
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

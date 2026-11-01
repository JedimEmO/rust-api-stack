# Versioning and Changelog Policy

This workspace uses independent crate versions. Bump only the publishable crates
whose public API, generated output, runtime behavior, or documented behavior
changed.

## Versioning

- Follow SemVer for crates at `1.0.0` and later.
- While crates are `0.x`, use patch bumps for backward-compatible fixes,
  additive behavior, docs, and tooling improvements.
- While crates are `0.x`, use minor bumps for breaking public API changes or
  generated-code contract changes.
- Do not version-bump examples, fixtures, or crates marked `publish = false`
  unless their version is meaningful to a downstream release process.
- Keep `Cargo.lock` aligned with any package version changes.
- All published crates share one `rust-version`, and CI builds them with exactly that toolchain.
  Raising it is a breaking change: a minor bump while crates are `0.x`.

## Changelog

- Every user-facing change must add an entry under `CHANGELOG.md` ->
  `[Unreleased]`.
- Group entries by `Added`, `Changed`, `Fixed`, `Documentation`, or
  `Maintenance`.
- Include the affected crate names when a change is scoped to specific crates.
- Mention version bumps in the same changelog entry set.

## Releasing

The [Release workflow](.github/workflows/release.yml) publishes every crate
whose version in `master` is not on crates.io yet, in dependency order,
and tags each one `<crate>-v<version>`, for example `ras-rest-macro-v0.4.1`.
It authenticates with crates.io trusted publishing, so the repository holds no API token.

1. Bump the versions of the crates that changed, and keep `Cargo.lock` aligned.
2. Move the `[Unreleased]` entries into a dated release section of `CHANGELOG.md`.
3. Merge to `master` with CI green.
4. Run the Release workflow from the Actions tab with `dry_run` checked.
   It lists the versions it would publish and verifies their packages.
5. Run it again with `dry_run` unchecked.
   The `publish` job runs in the `release` environment, which only `master` may deploy to.

### Publishing a crate for the first time

Trusted publishing only works for a crate that already exists on crates.io,
so the first version of a new crate is published by hand with an API token:

```bash
cargo publish -p <new-crate> --locked
```

Then register the Release workflow as its trusted publisher.
Create a crates.io API token with the `trusted-publishing` scope, and run:

```bash
CRATES_IO_TOKEN=... .github/scripts/configure-trusted-publishing.sh
```

The script configures every publishable crate that has no trusted publisher yet,
and skips the ones crates.io does not know.
Revoke the token afterwards; later releases do not need it.

# Release Binary Builds

TinyTop has an on-demand GitHub Actions workflow for building release binaries on native hosted runners.

## Workflow

Open:

```text
Actions -> Build release binaries -> Run workflow
```

The workflow file is:

```text
.github/workflows/build-binaries.yml
```

## Inputs

| Input | Required | Values | Purpose |
| --- | --- | --- | --- |
| `platform` | Yes | `all`, `linux`, `windows`, `macos` | Selects which platform family to build. |
| `release_tag` | No | Existing tag, for example `v0.1.34` | Release tag to attach assets to when release upload is enabled. |
| `upload_to_release` | Yes | `true` or `false` | When true, uploads built assets to `release_tag` with overwrite behavior. |

If `upload_to_release` is true, `release_tag` must be set and the release must already exist.

## Assets

| Platform input | Runner | Feature | Asset |
| --- | --- | --- | --- |
| `linux` | `ubuntu-24.04` | `linux-collector` | `tinytop-agent-linux-x86_64` |
| `windows` | `windows-2025` | `windows-collector` | `tinytop-agent-windows-x86_64.exe` |
| `macos` | `macos-15-intel` | `macos-collector` | `tinytop-agent-macos-x86_64` |
| `macos` | `macos-15` | `macos-collector` | `tinytop-agent-macos-aarch64` |

`platform=all` builds every row above.

Every asset has a sibling checksum file:

```text
<asset>.sha256
```

## Build behavior

Each matrix row:

1. Checks out the repository.
2. Selects stable Rust.
3. Builds `tinytop-agent` in release mode with exactly one platform collector feature.
4. Runs `tinytop-agent --help` as a binary smoke check.
5. Copies the binary into `dist/`.
6. Writes a SHA-256 checksum.
7. Uploads the binary and checksum as workflow artifacts.

## Attach assets to a release

To attach assets to an existing release:

1. Create or push the tag first.
2. Create the GitHub release for that tag.
3. Run `Build release binaries`.
4. Set `release_tag` to the tag, for example `v0.1.34`.
5. Set `upload_to_release` to `true`.

The workflow uses:

```bash
gh release upload "$release_tag" <asset> <asset>.sha256 --clobber
```

`--clobber` intentionally replaces an existing asset with the same name, which keeps reruns usable when a platform build needs to be regenerated for the same release tag.

## Notes

- The workflow is manual only; it does not run on every push.
- Windows and macOS binaries are built on native GitHub-hosted runners rather than cross-compiled from Linux.
- The workflow builds binaries, but it does not perform live Windows or macOS daemon/service verification.

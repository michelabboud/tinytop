# 0011 PowerShell-First Windows Command Center

## Context

TinyTop's default runtime is now the Rust collector/dashboard daemon, but the operator command center was Bash plus Linux user systemd. Windows needs a native bootstrap and lifecycle surface before package-manager distribution can be useful.

The options considered were:

- ship only the Rust daemon and require users to run it manually
- write a Rust installer first
- add Scoop/winget/MSI packaging first
- add a PowerShell command center first

## Decision

Add `tinytop.ps1` as the native Windows command center. It handles release-binary install, local Windows collector builds, foreground lifecycle, status, logs, and explicit Windows Service Control Manager commands.

Keep package-manager distribution as a follow-up after a real Windows release binary is built and uploaded. Scoop should be first because its manifest can point directly at GitHub release assets. winget and MSI/MSIX should follow after live-host verification and a clearer signing/service-install story.

## Alternatives Rejected

- Build the installer in Rust first. Rejected because bootstrap should work before a TinyTop binary exists.
- Start with Scoop or winget. Rejected because package managers do not replace the need for Windows paths, logs, PID/status handling, and service commands.
- Start with MSI/MSIX. Rejected because it adds signing and installer-maintenance weight before the Windows runtime has live-host parity.
- Use systemd on Windows through WSL. Rejected because native Windows support should not depend on WSL.

## Consequences

Windows users get a native command surface without Bun or GitHub Actions. `tinytop.ps1 service install` can install a real Windows service, but install/uninstall require an elevated PowerShell session. Full Windows support still needs a real Windows `.exe` release asset, live Windows host verification, and package-manager manifests.

## Status

Accepted.

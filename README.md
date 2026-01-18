# RunnerBuddy

RunnerBuddy is a Tauri desktop app that manages multiple GitHub Actions self-hosted runners with guided setup, secure secret storage, discovery/import, and per-runner service management.

## Features

- Multi-runner management with per-runner status and logs
- Guided setup wizard for repo/org/enterprise scopes
- PATs stored in the OS credential store (no plaintext secrets), with aliases
- Download and configure the official GitHub Actions runner
- Start/stop controls with live status per runner
- Run-on-boot service management (launchd/systemd) per runner
- Discovery/import of existing runner installs with optional service replacement
- Tray menu with quick start/stop/open controls
- Log viewer for app and runner diagnostics
- Cleanup flow with optional GitHub unregister

## Tech Stack

- Tauri 2
- Svelte (TypeScript) + Tailwind
- Rust backend commands

## Getting Started

### Prerequisites

- Node.js + npm
- Rust toolchain
- GitHub auth: GitHub CLI (`gh auth login`) or a GitHub PAT with required scopes

### Install dependencies

```bash
./runnerbuddy
```

### Run tests

```bash
npm run test
```

## Notes

- macOS/Linux: run `./runnerbuddy`
- Windows: run `runnerbuddy` (uses `runnerbuddy.cmd`)
- Service management defaults to user-level services for macOS (launchd) and Linux (systemd --user).
- Windows service support uses the runner's `svc.cmd` helper and may require admin privileges.

## Auto-updates (GitHub Releases)

RunnerBuddy can update itself using the Tauri updater plugin (update packages are signed with an updater keypair; the macOS app bundle itself does not need to be Apple-signed for updater verification).

1) Generate an updater signing keypair (keep the private key secret):

```bash
npm run tauri -- signer generate --ci --password "<choose-a-password>" --write-keys /tmp/runnerbuddy-tauri.key --force
cat /tmp/runnerbuddy-tauri.key.pub
```

2) In GitHub repo settings, add secrets:
- `TAURI_SIGNING_PRIVATE_KEY`: contents of `/tmp/runnerbuddy-tauri.key`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: the password you chose

3) Ensure `src-tauri/tauri.conf.json` has the updater `pubkey` set to the public key you generated.

4) Tag a release (workflow builds macOS Intel + Apple Silicon):

```bash
git tag v0.1.0
git push origin v0.1.0
```

## Security

- PATs are stored in the OS credential store (Keychain/Credential Manager/Secret Service).
- Registration tokens are fetched on-demand and never persisted.
- Logs are scrubbed of tokens where possible.

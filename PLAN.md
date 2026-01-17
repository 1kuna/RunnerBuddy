# RunnerBuddy — v0.2 Remaining Work

This file lists only the items from the original v0.2 plan that are **not yet implemented end-to-end** (including safety/rollback/acceptance criteria).

---

## 1) External Service Awareness (provider = external)

### 1.1 Persist accurate external service identity
- macOS: parse LaunchAgent plist and store:
  - the true `Label` value (not filename stem)
  - the plist path (so we can disable/unload reliably)
- Linux: store unit name + unit file path when detected.
- Windows: capture a stable service identity (beyond “svc.cmd exists”) when possible.

### 1.2 External service status + runner status integration
- Implement external service status (installed/running/enabled) and surface it in:
  - `service.status(_all)` results
  - `runner.status(_all)` “running” detection (so externally-run runners don’t show offline)
  - UI (provider badge + service state)

### 1.3 External artifact removal + reversibility record
- Add an explicit “Remove external artifacts” action (separate from “Replace service”).
- Record enough info to restore (what was disabled/unloaded and how).

**Acceptance**
- A runner running via an external LaunchAgent/systemd unit shows as running without any migration.
- Replacing a service is explicit and does not create conflicts.

---

## 2) Service Conflict Policy + UX Hardening

- Gate “Run on boot (RunnerBuddy)”:
  - Hide/disable the toggle when `provider=external` and show “Managed by external service” instead.
  - Require “Replace with RunnerBuddy” (or explicit override) before installing a RunnerBuddy service.
- Backend should refuse `service.install` when an external conflict is detected unless an explicit “force”/migration path is used.
- Improve conflict messaging: show exactly what will be disabled/unloaded, what will be installed, and how to undo.

---

## 3) Move → Verify → Delete Original (Managed Migration)

### 3.1 Stop/safety rules before move
- Ensure runner is stopped before copy:
  - stop app-spawned process
  - if external service is running, require explicit stop/disable first (or replace service first)

### 3.2 Migration metadata + rollback
- Persist migration state in config:
  - `install.adopted_from_path` (original location retained after move)
  - `install.migration_status` (`moved|verified|failed|...`)
- Optional: add rollback action (switch back to `adopted_from_path`) when verification fails.

### 3.3 Functional verification (not just file checks)
- After move, verify the runner actually runs from the new path:
  - start via RunnerBuddy service if enabled, else app-managed
  - confirm “connected/listening” via `_diag` markers with timeout
  - mark verified/failed and surface reason

### 3.4 Delete original as a separate explicit action
- Provide “Delete original install” only when:
  - verification succeeded
  - no service still references the original path
  - user confirms via a typed confirmation step

**Acceptance**
- “Import + move” leaves the runner functional; originals remain until a separate verified deletion step.

---

## 4) Cleanup/Unregister Correctness + Confirmation Hardening

- Implement proper GitHub “remove token” API endpoints per scope and use that for `config.sh remove`:
  - repo/org/enterprise remove-token endpoints (not registration-token)
- Strengthen destructive confirmations (typed) for:
  - unregister-from-GitHub
  - deleting install/work/log folders
  - replacing external services
  - deleting original installs after move

---

## 5) Discovery Hardening

- macOS:
  - external service detection must extract the true launchd label from the plist
  - disable/unload should use the correct label or plist-path semantics
- Candidate normalization:
  - broaden `.runner` parsing for variant schemas (labels object vs strings, url keys, etc.)
  - clearer “unknown scope” UX for operations that require scope/PAT (unregister/re-register)

---

## 6) Testing (Real + Safe)

### 6.1 Unit tests (Rust)
- External service parsing fixtures:
  - macOS LaunchAgent plist parsing (`Label`, program arguments)
  - Linux systemd unit parsing (ExecStart path detection)
- Migration metadata + gating rules:
  - cannot delete original unless verified
  - conflict gating for service install
- Remove-token endpoint selection per scope.

### 6.2 Integration tests (gated)
Only run when env vars are provided (PAT + scope):
- Create profile → download → configure → start → status transitions
- Verify flow (if implemented)
- Unregister/remove flow for a disposable test runner

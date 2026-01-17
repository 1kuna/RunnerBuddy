# RunnerBuddy — First-Launch Onboarding Wizard Plan

Goal: add a first-launch onboarding wizard that (1) recommends/controls auto-updates, and (2) guides users through adopting existing runners or migrating them into RunnerBuddy-managed installs with optional verify + delete of originals. Also add a “Re-run onboarding” entry so users can revisit choices later.

---

## 1) User Experience (UX)

### 1.1 Entry conditions
- On app launch:
  - If `onboarding.completed == false` → route to onboarding wizard.
  - If `onboarding.completed == true` → route to main dashboard.
- Add a Settings entry: **Re-run onboarding** (always available, requires confirmation).

### 1.2 Wizard steps (recommended flow)
1) **Welcome**
   - Explain: runners can run in background via OS services; RunnerBuddy is a dashboard and setup tool.
2) **Auto-updates**
   - Toggle: `Enable auto-updates` (recommended default `on`).
   - Toggle: `Auto-check on launch` (default `on`, only if auto-updates enabled).
   - Copy explaining unsigned app caveats (Gatekeeper warnings) vs updater-signature verification.
3) **Scan this machine**
   - Run discovery scan, show candidates.
   - Use “Adopt” terminology (avoid “Import”).
4) **Default adoption strategy**
   - Radio:
     - **Adopt in place** (no file move; manage runner where it is)
     - **Move → verify → optionally delete original** (RunnerBuddy-managed directory)
5) **Per-runner review**
   - For each discovered runner:
     - Strategy override per runner (adopt vs move/verify/delete).
     - Show external service detection and “Replace with RunnerBuddy service” option.
     - Show warnings if scope is unknown (limits unregister operations).
6) **Execute**
   - Progress display per runner: adopt/move/verify/service migration.
   - Clear outcomes: success, needs action, failed (with safe rollback).
7) **Finish**
   - Mark onboarding complete.
   - Route to main dashboard with the selected runner highlighted.

### 1.3 Re-run onboarding (Settings)
- Add UI: Settings → **Re-run onboarding**
  - Confirmation modal: “This will re-open onboarding and may change defaults; existing runners are not modified unless you choose actions.”
  - Behavior:
    - Does not delete anything automatically.
    - Lets user reconfigure update preferences and optionally scan/adopt/migrate more runners.
  - Optional: “Reset onboarding completion” button that sets `onboarding.completed=false` and navigates to onboarding route.

---

## 2) Config + State Model Changes (Backend)

### 2.1 New config fields (schema bump)
Add to config (no secrets):
- `schema_version: 3` (migrate from v2)
- `onboarding`:
  - `completed: bool` (default `false` for new installs; `true` when upgraded from prior versions to avoid surprising existing users)
  - `completed_at: string | null`
- `settings`:
  - `auto_updates_enabled: bool` (default `true`)
  - `auto_check_updates_on_launch: bool` (default `true`)
  - `adoption_default: "adopt" | "move_verify_delete"` (default `"adopt"`)

### 2.2 Per-runner migration tracking
Extend runner profile install metadata:
- `install.mode: "managed" | "adopted"` (already exists)
- `install.install_path: string` (already exists)
- `install.adopted_from_path?: string | null`
  - Set when a runner is moved into managed dir (store original path).
- `install.migration_status?: "none" | "moved" | "verified" | "failed"`
  - Used to gate “delete original” action.

### 2.3 Migration rules
- Never delete or unregister without explicit user confirmation.
- If “move” happens:
  - Copy → verify copy → switch profile path.
  - Keep `adopted_from_path` until user explicitly deletes.
- If service is RunnerBuddy-managed:
  - Ensure service files point at the new path after move.

---

## 3) Backend API (Tauri Commands)

### 3.1 Settings + onboarding
- `settings.get()` → `{ onboarding, settings }`
- `settings.update(patch)` → updated `{ onboarding, settings }`
- `onboarding.complete()` → sets `onboarding.completed=true`, `completed_at=now`
- `onboarding.reset()` → sets `onboarding.completed=false` (used by “Re-run onboarding”)

### 3.2 Discovery/adoption flows (extend existing)
Keep existing discovery scan but rename in UI:
- `discover.scan()` → candidates (existing)
- `discover.adopt(candidate_id, options)` → runner_id
  - `options.strategy: "adopt" | "move_verify_delete"`
  - `options.replace_service: bool`
  - `options.delete_original_after_verify: bool`

### 3.3 Verify + delete original (new)
- `discover.verify_runner(runner_id)` → `{ ok: bool, reason?: string }`
  - Implements “verify” definition (see section 4).
- `discover.delete_original_install(runner_id)` → deletes `install.adopted_from_path`
  - Requires:
    - `install.migration_status == "verified"`
    - and no known external service still referencing old path
    - and user confirmation handled in UI (typed prompt)

### 3.4 Optional: rollback support (nice-to-have)
- `discover.rollback_move(runner_id)` → restore profile to `adopted_from_path` if still present.

---

## 4) “Verify” Definition (Must be Safe + Deterministic)

Verification should confirm the moved runner actually works from the new path before deletion is allowed.

Proposed verification procedure:
1) Stop the runner process (if app-started) and stop RunnerBuddy service (if RunnerBuddy-managed).
2) Start runner from the new path:
   - Prefer RunnerBuddy service if enabled; otherwise start as app-managed foreground process.
3) Wait up to N seconds and inspect `_diag` logs for a successful “online/connected/listening” marker.
4) If marker found → set `migration_status="verified"`.
5) On failure → set `migration_status="failed"` and keep original path untouched.

Note: verification should avoid needing GitHub API if possible; if scope + PAT is available, optional API checks can be added later.

---

## 5) Frontend Implementation Plan (Svelte)

### 5.1 Routing + gating
- Add route: `src/routes/onboarding/+page.svelte`
- Add a guard in layout init:
  - load `settings.get()` (or full config state)
  - if `!onboarding.completed` → navigate to `/onboarding`

### 5.2 Settings entry
- Add “Settings” panel or menu in main UI:
  - **Re-run onboarding** button → calls `onboarding.reset()` and navigates to `/onboarding`
  - Also expose update toggles here so onboarding isn’t the only place to change them.

### 5.3 Onboarding UI components
- Stepper UI + per-step forms
- Candidate list with per-runner strategy overrides
- Progress UI for execute step (events or polling)
- Confirmation dialogs for delete-original actions (typed confirmation)

---

## 6) Testing Plan

### 6.1 Rust unit tests
- Config migration v2 → v3 defaults (ensure upgrades don’t unexpectedly force onboarding)
- Verify “delete original” gating rules (cannot delete without verified status)
- Discovery metadata parsing fixtures (existing tests + more cases)

### 6.2 Frontend checks
- `npm run check` (type + Svelte diagnostics)
- Minimal UI state test coverage where feasible (or keep as manual QA if no existing framework)

### 6.3 Manual QA (macOS first)
- Fresh install: onboarding appears; completes; does not reappear
- “Re-run onboarding”: appears and does not break existing runner profiles
- Adopt flow: no file moves; runner still works
- Move/verify flow: verify gates deletion; deleting original removes old folder only after verification
- External service replacement: safe/explicit; does not delete external artifacts by default
- Auto-update toggles: enabled/disabled behavior matches choices; errors only shown for manual checks

---

## 7) Acceptance Criteria

- On first launch, onboarding wizard appears and guides the user through update preferences + discovery/adoption.
- Auto-update checking respects user preferences:
  - If disabled, no checks occur.
  - If enabled + auto-check enabled, a silent check runs on launch.
- “Adopt in place” never moves or deletes runner installs.
- “Move → verify → delete” never deletes anything until verification succeeds and user explicitly confirms.
- A Settings entry exists to re-run onboarding safely without forcing destructive actions.


<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import {
    completeOnboarding,
    discoverDeleteOriginalInstall,
    discoverImport,
    discoverScan,
    discoverVerifyRunner,
    getSettings,
    selectRunner,
    updateSettings,
    type AdoptionDefault,
    type DiscoveryCandidate,
    type RunnerScope,
    type SettingsSnapshot
  } from "$lib/api";

  type CandidateChoice = {
    include: boolean;
    strategy: AdoptionDefault;
    replaceService: boolean;
    deleteOriginalAfterVerify: boolean;
  };

  type ExecuteStatus = {
    status: "pending" | "running" | "success" | "needs_action" | "failed";
    detail: string;
    runnerId?: string;
  };

  const stepTitles = [
    "Welcome",
    "Auto-updates",
    "Scan this machine",
    "Default adoption",
    "Per-runner review",
    "Execute",
    "Finish",
  ];

  let step = $state(1);
  let errorMessage = $state<string | null>(null);
  let isBusy = $state(false);

  let settingsSnapshot = $state<SettingsSnapshot | null>(null);
  let autoUpdatesEnabled = $state(true);
  let autoCheckOnLaunch = $state(true);
  let adoptionDefault = $state<AdoptionDefault>("adopt");

  let discoveryCandidates = $state<DiscoveryCandidate[]>([]);
  let isScanning = $state(false);
  let candidateOptions = $state<Record<string, CandidateChoice>>({});

  let executeStates = $state<Record<string, ExecuteStatus>>({});
  let executeStarted = $state(false);
  let lastRunnerId = $state<string | null>(null);
  let lastRunnerLabel = $state<string | null>(null);

  onMount(() => {
    let cancelled = false;
    void (async () => {
      try {
        settingsSnapshot = await getSettings();
        if (cancelled || !settingsSnapshot) return;
        autoUpdatesEnabled = settingsSnapshot.settings.auto_updates_enabled;
        autoCheckOnLaunch = settingsSnapshot.settings.auto_check_updates_on_launch;
        adoptionDefault = settingsSnapshot.settings.adoption_default;
      } catch (error) {
        errorMessage = `${error}`;
      }
    })();
    return () => {
      cancelled = true;
    };
  });

  $effect(() => {
    if (!autoUpdatesEnabled) {
      autoCheckOnLaunch = false;
    }
  });

  function scopeLabel(scope?: RunnerScope | null): string {
    if (!scope) return "Scope unknown";
    if (scope.type === "repo") return `${scope.owner}/${scope.repo}`;
    if (scope.type === "org") return scope.org;
    return scope.enterprise;
  }

  function requireTypedConfirm(message: string, expected: string): boolean {
    const input = prompt(`${message}\\nType \"${expected}\" to confirm.`);
    return input === expected;
  }

  function initCandidateOptions(candidates: DiscoveryCandidate[]) {
    const next: Record<string, CandidateChoice> = {};
    for (const candidate of candidates) {
      next[candidate.candidate_id] = {
        include: true,
        strategy: adoptionDefault,
        replaceService: false,
        deleteOriginalAfterVerify: false,
      };
    }
    candidateOptions = next;
  }

  function updateCandidateOption(candidateId: string, patch: Partial<CandidateChoice>) {
    const current = candidateOptions[candidateId];
    if (!current) return;
    candidateOptions[candidateId] = { ...current, ...patch };
  }

  function hasIncludedCandidates(): boolean {
    return Object.values(candidateOptions).some((option) => option.include);
  }

  function formatExecuteStatus(status?: ExecuteStatus["status"]): string {
    if (!status) return "pending";
    return status === "needs_action" ? "needs action" : status;
  }

  async function handleSaveUpdates() {
    errorMessage = null;
    isBusy = true;
    try {
      settingsSnapshot = await updateSettings({
        auto_updates_enabled: autoUpdatesEnabled,
        auto_check_updates_on_launch: autoUpdatesEnabled ? autoCheckOnLaunch : false,
      });
      if (settingsSnapshot) {
        autoUpdatesEnabled = settingsSnapshot.settings.auto_updates_enabled;
        autoCheckOnLaunch = settingsSnapshot.settings.auto_check_updates_on_launch;
      }
      step = 3;
    } catch (error) {
      errorMessage = `${error}`;
    } finally {
      isBusy = false;
    }
  }

  async function handleSaveAdoptionDefault() {
    errorMessage = null;
    isBusy = true;
    try {
      settingsSnapshot = await updateSettings({
        adoption_default: adoptionDefault,
      });
      for (const candidate of discoveryCandidates) {
        if (candidateOptions[candidate.candidate_id]) {
          candidateOptions[candidate.candidate_id].strategy = adoptionDefault;
        }
      }
      step = 5;
    } catch (error) {
      errorMessage = `${error}`;
    } finally {
      isBusy = false;
    }
  }

  async function handleScan() {
    errorMessage = null;
    isScanning = true;
    try {
      discoveryCandidates = await discoverScan();
      initCandidateOptions(discoveryCandidates);
    } catch (error) {
      errorMessage = `${error}`;
    } finally {
      isScanning = false;
    }
  }

  async function handleExecute() {
    if (executeStarted) return;
    errorMessage = null;
    executeStarted = true;
    isBusy = true;
    executeStates = {};
    lastRunnerId = null;
    lastRunnerLabel = null;
    try {
      for (const candidate of discoveryCandidates) {
        const options = candidateOptions[candidate.candidate_id];
        if (!options?.include) continue;
        executeStates[candidate.candidate_id] = {
          status: "running",
          detail: "Adopting runner",
        };
        try {
          const runnerId = await discoverImport(candidate.candidate_id, {
            replace_service: options.replaceService,
            move_install: options.strategy === "move_verify_delete",
            verify_after_move: false,
            delete_original_after_verify: false,
          });
          const adoptDetail = options.replaceService ? "Adopted and service replaced" : "Adopted";
          executeStates[candidate.candidate_id] = {
            status: "running",
            detail:
              options.strategy === "move_verify_delete" ? "Verifying moved install" : adoptDetail,
            runnerId,
          };
          lastRunnerId = runnerId;
          lastRunnerLabel = candidate.runner_name ?? candidate.install_path;

          if (options.strategy === "move_verify_delete") {
            const verify = await discoverVerifyRunner(runnerId);
            if (!verify.ok) {
              executeStates[candidate.candidate_id] = {
                status: "needs_action",
                detail: verify.reason ?? "Verification failed",
                runnerId,
              };
              continue;
            }

            if (options.deleteOriginalAfterVerify) {
              const expected = candidate.runner_name ?? "delete";
              if (!requireTypedConfirm("Delete the original runner install?", expected)) {
                executeStates[candidate.candidate_id] = {
                  status: "needs_action",
                  detail: "Deletion skipped by user",
                  runnerId,
                };
                continue;
              }
              executeStates[candidate.candidate_id] = {
                status: "running",
                detail: "Deleting original install",
                runnerId,
              };
              try {
                await discoverDeleteOriginalInstall(runnerId);
                executeStates[candidate.candidate_id] = {
                  status: "success",
                  detail: options.replaceService
                    ? "Moved, verified, service replaced, and original deleted"
                    : "Moved, verified, and original deleted",
                  runnerId,
                };
              } catch (error) {
                executeStates[candidate.candidate_id] = {
                  status: "needs_action",
                  detail: `${error}`,
                  runnerId,
                };
              }
            } else {
              executeStates[candidate.candidate_id] = {
                status: "success",
                detail: options.replaceService
                  ? "Moved, verified, and service replaced"
                  : "Moved and verified",
                runnerId,
              };
            }
          } else {
            executeStates[candidate.candidate_id] = {
              status: "success",
              detail: options.replaceService
                ? "Adopted in place and service replaced"
                : "Adopted in place",
              runnerId,
            };
          }
        } catch (error) {
          executeStates[candidate.candidate_id] = {
            status: "failed",
            detail: `${error}`,
          };
        }
      }
    } finally {
      isBusy = false;
    }
  }

  async function handleFinish() {
    errorMessage = null;
    isBusy = true;
    try {
      await completeOnboarding();
      if (lastRunnerId) {
        await selectRunner(lastRunnerId);
      }
      await goto("/");
    } catch (error) {
      errorMessage = `${error}`;
    } finally {
      isBusy = false;
    }
  }
</script>

<main class="min-h-screen px-6 py-10 text-slate-100">
  <div class="mx-auto max-w-5xl space-y-8">
    <header class="flex flex-col gap-4 rounded-3xl px-8 py-8 glass-panel">
      <p class="text-sm uppercase tracking-[0.2em] text-slate-400">RunnerBuddy onboarding</p>
      <h1 class="font-display text-3xl text-white">Set your defaults and bring existing runners under control.</h1>
      <p class="max-w-3xl text-sm text-slate-300">
        This guided flow helps you set update preferences, discover existing runners, and decide how to adopt them safely.
      </p>
    </header>

    {#if errorMessage}
      <div class="rounded-2xl border border-red-400/30 bg-red-500/10 px-4 py-3 text-sm text-red-100">
        {errorMessage}
      </div>
    {/if}

    <div class="grid gap-6 lg:grid-cols-[240px_1fr]">
      <aside class="space-y-3">
        {#each stepTitles as title, index}
          <div
            class={`rounded-xl border px-3 py-2 text-sm ${
              step === index + 1
                ? "border-tide-400/60 bg-tide-500/10 text-white"
                : step > index + 1
                  ? "border-emerald-400/60 bg-emerald-500/10 text-emerald-100"
                  : "border-slate-500/40 text-slate-300"
            }`}
          >
            {index + 1}. {title}
          </div>
        {/each}
      </aside>

      <section class="rounded-2xl px-6 py-6 glass-panel">
        {#if step === 1}
          <div class="space-y-4">
            <h2 class="text-2xl font-display text-white">Welcome</h2>
            <p class="text-sm text-slate-300">
              RunnerBuddy manages runners and their services. It can adopt installs already on your machine or move them
              into a RunnerBuddy-managed folder.
            </p>
            <div class="rounded-xl border border-slate-500/30 bg-slate-950/40 px-4 py-3 text-sm text-slate-200">
              <p class="font-semibold text-white">What happens in this wizard</p>
              <ul class="mt-2 space-y-1 text-xs text-slate-300">
                <li>Set update preferences and auto-check behavior.</li>
                <li>Scan this machine for existing GitHub Actions runners.</li>
                <li>Choose how each runner is adopted and whether to migrate services.</li>
              </ul>
            </div>
            <button
              class="rounded-xl bg-tide-500 px-4 py-2 text-sm font-semibold text-white"
              onclick={() => (step = 2)}
            >
              Continue
            </button>
          </div>
        {/if}

        {#if step === 2}
          <div class="space-y-4">
            <h2 class="text-2xl font-display text-white">Auto-updates</h2>
            <p class="text-sm text-slate-300">
              Updates are signed and verified by the updater. macOS Gatekeeper may still warn on first launch if the app
              is unsigned.
            </p>
            <div class="space-y-3">
              <label class="flex items-center justify-between rounded-xl border border-slate-500/40 px-4 py-3 text-sm">
                <span class="text-slate-200">Enable auto-updates</span>
                <input
                  type="checkbox"
                  class="rounded border-slate-500 bg-transparent text-tide-500 focus:ring-tide-500"
                  bind:checked={autoUpdatesEnabled}
                />
              </label>
              <label
                class={`flex items-center justify-between rounded-xl border px-4 py-3 text-sm ${
                  autoUpdatesEnabled ? "border-slate-500/40" : "border-slate-600/30 opacity-60"
                }`}
              >
                <span class="text-slate-200">Auto-check on launch</span>
                <input
                  type="checkbox"
                  class="rounded border-slate-500 bg-transparent text-tide-500 focus:ring-tide-500"
                  bind:checked={autoCheckOnLaunch}
                  disabled={!autoUpdatesEnabled}
                />
              </label>
              <p class="text-xs text-slate-400">
                RunnerBuddy never installs updates without consent. Auto-checking only fetches update availability.
              </p>
            </div>
            <div class="flex flex-wrap gap-3">
              <button
                class="rounded-xl border border-slate-500/40 px-4 py-2 text-sm font-semibold text-slate-200"
                onclick={() => (step = 1)}
              >
                Back
              </button>
              <button
                class="rounded-xl bg-tide-500 px-4 py-2 text-sm font-semibold text-white"
                onclick={handleSaveUpdates}
                disabled={isBusy}
              >
                Continue
              </button>
            </div>
          </div>
        {/if}

        {#if step === 3}
          <div class="space-y-4">
            <h2 class="text-2xl font-display text-white">Scan this machine</h2>
            <p class="text-sm text-slate-300">
              RunnerBuddy will look for GitHub Actions runner folders in common locations. You can adopt them in place or
              move them later.
            </p>
            <button
              class="rounded-xl bg-tide-500 px-4 py-2 text-sm font-semibold text-white"
              onclick={handleScan}
              disabled={isScanning}
            >
              {isScanning ? "Scanning..." : "Scan now"}
            </button>

            {#if discoveryCandidates.length}
              <div class="space-y-3">
                {#each discoveryCandidates as candidate}
                  <div class="rounded-xl border border-slate-500/40 bg-slate-950/40 px-4 py-3 text-sm">
                    <p class="font-semibold text-white">{candidate.runner_name ?? "Unknown runner"}</p>
                    <p class="text-xs text-slate-400">{candidate.install_path}</p>
                    <p class="text-xs text-slate-500">Scope: {scopeLabel(candidate.scope ?? null)}</p>
                    {#if candidate.service_present}
                      <p class="text-xs text-slate-500">
                        Service detected: {candidate.service_id ?? "external"}
                      </p>
                    {/if}
                  </div>
                {/each}
              </div>
            {:else if !isScanning}
              <p class="text-sm text-slate-400">No candidates yet. You can continue without adopting.</p>
            {/if}

            <div class="flex flex-wrap gap-3">
              <button
                class="rounded-xl border border-slate-500/40 px-4 py-2 text-sm font-semibold text-slate-200"
                onclick={() => (step = 2)}
              >
                Back
              </button>
              <button
                class="rounded-xl bg-tide-500 px-4 py-2 text-sm font-semibold text-white"
                onclick={() => (step = 4)}
                disabled={isScanning}
              >
                Continue
              </button>
            </div>
          </div>
        {/if}

        {#if step === 4}
          <div class="space-y-4">
            <h2 class="text-2xl font-display text-white">Default adoption strategy</h2>
            <p class="text-sm text-slate-300">
              Choose how RunnerBuddy should adopt existing runners by default. You can override this per runner next.
            </p>
            <div class="space-y-3">
              <label class="flex items-start gap-3 rounded-xl border border-slate-500/40 px-4 py-3 text-sm">
                <input
                  type="radio"
                  name="adoption-default"
                  class="mt-1"
                  value="adopt"
                  bind:group={adoptionDefault}
                />
                <span>
                  <span class="font-semibold text-white">Adopt in place</span>
                  <span class="block text-xs text-slate-400">
                    Keep runners where they are and manage them in place. No files are moved.
                  </span>
                </span>
              </label>
              <label class="flex items-start gap-3 rounded-xl border border-slate-500/40 px-4 py-3 text-sm">
                <input
                  type="radio"
                  name="adoption-default"
                  class="mt-1"
                  value="move_verify_delete"
                  bind:group={adoptionDefault}
                />
                <span>
                  <span class="font-semibold text-white">Move, verify, then optionally delete</span>
                  <span class="block text-xs text-slate-400">
                    Copy the runner into RunnerBuddy-managed storage, verify it starts, and choose whether to delete the
                    original.
                  </span>
                </span>
              </label>
            </div>
            <div class="flex flex-wrap gap-3">
              <button
                class="rounded-xl border border-slate-500/40 px-4 py-2 text-sm font-semibold text-slate-200"
                onclick={() => (step = 3)}
              >
                Back
              </button>
              <button
                class="rounded-xl bg-tide-500 px-4 py-2 text-sm font-semibold text-white"
                onclick={handleSaveAdoptionDefault}
                disabled={isBusy}
              >
                Continue
              </button>
            </div>
          </div>
        {/if}

        {#if step === 5}
          <div class="space-y-4">
            <h2 class="text-2xl font-display text-white">Per-runner review</h2>
            <p class="text-sm text-slate-300">
              Confirm how each runner should be adopted. RunnerBuddy will not delete anything without explicit action.
            </p>

            {#if discoveryCandidates.length === 0}
              <p class="text-sm text-slate-400">No runners found. You can continue to finish onboarding.</p>
            {/if}

            <div class="space-y-4">
              {#each discoveryCandidates as candidate}
                <div class="rounded-xl border border-slate-500/40 bg-slate-950/40 px-4 py-4 space-y-3">
                  <div class="flex flex-wrap items-center justify-between gap-3">
                    <div>
                      <p class="text-sm font-semibold text-white">{candidate.runner_name ?? "Unknown runner"}</p>
                      <p class="text-xs text-slate-400">{candidate.install_path}</p>
                      <p class="text-xs text-slate-500">Scope: {scopeLabel(candidate.scope ?? null)}</p>
                    </div>
                    <label class="flex items-center gap-2 text-xs text-slate-300">
                      <input
                        type="checkbox"
                        class="rounded border-slate-500 bg-transparent text-tide-500"
                        checked={candidateOptions[candidate.candidate_id]?.include ?? true}
                        onchange={(event) =>
                          updateCandidateOption(candidate.candidate_id, {
                            include: (event.target as HTMLInputElement).checked,
                          })
                        }
                      />
                      Include
                    </label>
                  </div>

                  {#if !candidate.scope}
                    <p class="text-xs text-amber-200">
                      Scope unknown. RunnerBuddy can adopt locally, but unregistering later will require setting scope.
                    </p>
                  {/if}

                  <div class="grid gap-3 md:grid-cols-2">
                    <label class="flex items-start gap-3 rounded-xl border border-slate-500/40 px-3 py-2 text-xs">
                      <input
                        type="radio"
                        name={`strategy-${candidate.candidate_id}`}
                        class="mt-1"
                        checked={candidateOptions[candidate.candidate_id]?.strategy === "adopt"}
                        onchange={() => updateCandidateOption(candidate.candidate_id, { strategy: "adopt" })}
                      />
                      <span>
                        <span class="font-semibold text-white">Adopt in place</span>
                        <span class="block text-slate-400">Manage it where it is.</span>
                      </span>
                    </label>
                    <label class="flex items-start gap-3 rounded-xl border border-slate-500/40 px-3 py-2 text-xs">
                      <input
                        type="radio"
                        name={`strategy-${candidate.candidate_id}`}
                        class="mt-1"
                        checked={candidateOptions[candidate.candidate_id]?.strategy === "move_verify_delete"}
                        onchange={() => {
                          updateCandidateOption(candidate.candidate_id, {
                            strategy: "move_verify_delete",
                            ...(candidate.service_present ? { replaceService: true } : {}),
                          });
                        }}
                      />
                      <span>
                        <span class="font-semibold text-white">Move + verify</span>
                        <span class="block text-slate-400">Copy into RunnerBuddy-managed storage.</span>
                      </span>
                    </label>
                  </div>

                  {#if candidate.service_present}
                    <label class="flex items-center justify-between rounded-xl border border-slate-500/40 px-3 py-2 text-xs">
                      <span class="text-slate-300">Replace external service with RunnerBuddy</span>
                      <input
                        type="checkbox"
                        class="rounded border-slate-500 bg-transparent text-tide-500"
                        checked={(candidateOptions[candidate.candidate_id]?.strategy === "move_verify_delete") || (candidateOptions[candidate.candidate_id]?.replaceService ?? false)}
                        onchange={(event) =>
                          updateCandidateOption(candidate.candidate_id, {
                            replaceService: (event.target as HTMLInputElement).checked,
                          })
                        }
                        disabled={candidateOptions[candidate.candidate_id]?.strategy === "move_verify_delete"}
                      />
                    </label>
                    {#if candidateOptions[candidate.candidate_id]?.strategy === "move_verify_delete"}
                      <p class="text-xs text-slate-400">
                        Move requires replacing the external service so it can point at the new path.
                      </p>
                    {:else if !candidateOptions[candidate.candidate_id]?.replaceService}
                      <p class="text-xs text-amber-200">
                        External service remains active. Move and delete-original operations will be blocked until the
                        external service is replaced or removed.
                      </p>
                    {/if}
                  {/if}

                  {#if candidateOptions[candidate.candidate_id]?.strategy === "move_verify_delete"}
                    <label class="flex items-center justify-between rounded-xl border border-slate-500/40 px-3 py-2 text-xs">
                      <span class="text-slate-300">Delete original after verify</span>
                      <input
                        type="checkbox"
                        class="rounded border-slate-500 bg-transparent text-tide-500"
                        checked={candidateOptions[candidate.candidate_id]?.deleteOriginalAfterVerify ?? false}
                        onchange={(event) =>
                          updateCandidateOption(candidate.candidate_id, {
                            deleteOriginalAfterVerify: (event.target as HTMLInputElement).checked,
                          })
                        }
                      />
                    </label>
                    <p class="text-xs text-slate-400">
                      Original folders are deleted only after successful verification.
                    </p>
                  {/if}
                </div>
              {/each}
            </div>

            <div class="flex flex-wrap gap-3">
              <button
                class="rounded-xl border border-slate-500/40 px-4 py-2 text-sm font-semibold text-slate-200"
                onclick={() => (step = 4)}
              >
                Back
              </button>
              <button
                class="rounded-xl bg-tide-500 px-4 py-2 text-sm font-semibold text-white"
                onclick={() => (step = 6)}
              >
                Continue
              </button>
            </div>
          </div>
        {/if}

        {#if step === 6}
          <div class="space-y-4">
            <h2 class="text-2xl font-display text-white">Execute adoption</h2>
            <p class="text-sm text-slate-300">
              RunnerBuddy will adopt the selected runners. Verification runs before any optional deletion.
            </p>

            {#if !hasIncludedCandidates()}
              <p class="text-sm text-slate-400">No runners selected for adoption.</p>
            {/if}

            <button
              class="rounded-xl bg-tide-500 px-4 py-2 text-sm font-semibold text-white"
              onclick={handleExecute}
              disabled={isBusy || executeStarted || !hasIncludedCandidates()}
            >
              {executeStarted ? "Executed" : "Run adoption"}
            </button>

            {#if executeStarted}
              <div class="space-y-3">
                {#each discoveryCandidates as candidate}
                  {#if candidateOptions[candidate.candidate_id]?.include}
                    <div class="rounded-xl border border-slate-500/40 bg-slate-950/40 px-4 py-3 text-sm">
                      <div class="flex items-center justify-between">
                        <span class="font-semibold text-white">{candidate.runner_name ?? "Unknown runner"}</span>
                        <span class="text-xs text-slate-400">
                          {formatExecuteStatus(executeStates[candidate.candidate_id]?.status)}
                        </span>
                      </div>
                      <p class="text-xs text-slate-400">
                        {executeStates[candidate.candidate_id]?.detail ?? "Pending"}
                      </p>
                    </div>
                  {/if}
                {/each}
              </div>
            {/if}

            <div class="flex flex-wrap gap-3">
              <button
                class="rounded-xl border border-slate-500/40 px-4 py-2 text-sm font-semibold text-slate-200"
                onclick={() => (step = 5)}
              >
                Back
              </button>
              <button
                class="rounded-xl bg-tide-500 px-4 py-2 text-sm font-semibold text-white"
                onclick={() => (step = 7)}
                disabled={isBusy}
              >
                Continue
              </button>
            </div>
          </div>
        {/if}

        {#if step === 7}
          <div class="space-y-4">
            <h2 class="text-2xl font-display text-white">Finish</h2>
            <p class="text-sm text-slate-300">
              Onboarding is complete. You can revisit these settings at any time from the Settings panel.
            </p>
            {#if lastRunnerLabel}
              <div class="rounded-xl border border-emerald-400/40 bg-emerald-500/10 px-4 py-3 text-sm text-emerald-100">
                Highlighting: {lastRunnerLabel}
              </div>
            {/if}
            <button
              class="rounded-xl bg-tide-500 px-4 py-2 text-sm font-semibold text-white"
              onclick={handleFinish}
              disabled={isBusy}
            >
              Go to dashboard
            </button>
          </div>
        {/if}
      </section>
    </div>
  </div>
</main>

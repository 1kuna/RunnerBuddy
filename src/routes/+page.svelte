<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { listen } from "@tauri-apps/api/event";
  import { getVersion } from "@tauri-apps/api/app";
  import { check } from "@tauri-apps/plugin-updater";
  import { relaunch } from "@tauri-apps/plugin-process";
  import {
    checkPat,
    clearPat,
    configureRunner,
    createRunnerProfile,
    deleteRunnerProfile,
    discoverDeleteOriginalInstall,
    discoverImport,
    discoverMigrateService,
    discoverRemoveExternalArtifacts,
    discoverMoveInstall,
    discoverScan,
    discoverVerifyRunner,
    downloadRunner,
    fetchRunnerStatus,
    fetchRunnerStatusAll,
    fetchServiceStatus,
    fetchServiceStatusAll,
    getSettings,
    installService,
    listLogSources,
    runnersList,
    resetOnboarding,
    savePat,
    selectRunner,
    setDefaultPatAlias,
    setRunOnBoot,
    startRunner,
    stopRunner,
    tailLogs,
    updateSettings,
    updateRunnerProfile,
    type AdoptionDefault,
    type AppSnapshot,
    type DiscoveryCandidate,
    type LogLine,
    type LogSource,
    type ProgressPayload,
    type RunnerProfile,
    type RunnerScope,
    type RunnerStatus,
    type ServiceStatus,
    type SettingsSnapshot
  } from "$lib/api";

  type RunnerStatusPayload = {
    runner_id: string;
    status: RunnerStatus;
    pid?: number | null;
    last_heartbeat?: number | null;
  };

  let snapshot = $state<AppSnapshot | null>(null);
  let serviceStatusMap = $state<Record<string, ServiceStatus>>({});
  let errorMessage = $state<string | null>(null);
  let isBusy = $state(false);
  let progress = $state<ProgressPayload | null>(null);

  let selectedRunnerId = $state<string | null>(null);
  let selectedLogSource = $state("app");
  let logSources = $state<LogSource[]>([]);
  let logLines = $state<LogLine[]>([]);

  let showCreate = $state(false);
  let wizardStep = $state(1);
  let createdRunnerId = $state<string | null>(null);

  let patAlias = $state("default");
  let patInput = $state("");
  let patValid = $state(false);

  let displayName = $state("");
  let scopeType = $state<"repo" | "org" | "enterprise">("repo");
  let scopeOwner = $state("");
  let scopeRepo = $state("");
  let scopeOrg = $state("");
  let scopeEnterprise = $state("");

  let runnerName = $state("");
  let runnerLabels = $state("");
  let workDir = $state("");

  let discoveryCandidates = $state<DiscoveryCandidate[]>([]);
  let isScanning = $state(false);

  let statusTimer: number | undefined;
  let logsTimer: number | undefined;

  let appVersion = $state<string | null>(null);
  let updateCheckBusy = $state(false);
  let updateInstallBusy = $state(false);
  let updateError = $state<string | null>(null);
  let pendingUpdate = $state<any | null>(null);
  let settingsSnapshot = $state<SettingsSnapshot | null>(null);
  let settingsBusy = $state(false);
  let settingsError = $state<string | null>(null);
  let settingsLoaded = $state(false);
  let autoUpdatesEnabled = $state(true);
  let autoCheckOnLaunch = $state(true);
  let adoptionDefault = $state<AdoptionDefault>("adopt");

  const stepTitles = [
    "Save access token",
    "Select scope",
    "Runner settings",
    "Download & configure",
    "Start runner",
  ];

  function buildScope(): RunnerScope {
    if (scopeType === "repo") {
      return { type: "repo", owner: scopeOwner.trim(), repo: scopeRepo.trim() };
    }
    if (scopeType === "org") {
      return { type: "org", org: scopeOrg.trim() };
    }
    return { type: "enterprise", enterprise: scopeEnterprise.trim() };
  }

  function labelsArray(): string[] {
    return runnerLabels
      .split(",")
      .map((label) => label.trim())
      .filter(Boolean);
  }

  function selectedRunner(): RunnerProfile | null {
    if (!snapshot || !selectedRunnerId) return null;
    return snapshot.config.runners.find((runner) => runner.runner_id === selectedRunnerId) ?? null;
  }

  function requireTypedConfirm(message: string, expected: string): boolean {
    const input = prompt(`${message}\nType \"${expected}\" to confirm.`);
    return input === expected;
  }

  function scopeLabel(scope?: RunnerScope | null): string {
    if (!scope) return "Scope unknown";
    if (scope.type === "repo") return `${scope.owner}/${scope.repo}`;
    if (scope.type === "org") return scope.org;
    return scope.enterprise;
  }

  function migrationStatus(runner: RunnerProfile | null): string {
    if (!runner) return "none";
    return runner.install.migration_status ?? "none";
  }

  function hasMigration(runner: RunnerProfile | null): boolean {
    if (!runner) return false;
    return !!runner.install.adopted_from_path || migrationStatus(runner) !== "none";
  }

  function canDeleteOriginal(runner: RunnerProfile | null): boolean {
    if (!runner) return false;
    return runner.install.migration_status === "verified" && !!runner.install.adopted_from_path;
  }

  function runnerRuntime(runnerId: string | null): RunnerStatus | null {
    if (!snapshot || !runnerId) return null;
    return snapshot.runtime[runnerId]?.status ?? null;
  }

  async function refreshState() {
    snapshot = await runnersList();
    if (snapshot?.config.selected_runner_id) {
      selectedRunnerId ||= snapshot.config.selected_runner_id;
    }
    if (snapshot?.config.pat_default_alias) {
      patAlias ||= snapshot.config.pat_default_alias;
    }
    serviceStatusMap = await fetchServiceStatusAll();
    if (selectedRunnerId) {
      await selectRunner(selectedRunnerId);
    }
  }

  async function refreshSelectedStatus() {
    if (!selectedRunnerId) return;
    const runtime = await fetchRunnerStatus(selectedRunnerId);
    snapshot = snapshot
      ? { ...snapshot, runtime: { ...snapshot.runtime, [selectedRunnerId]: runtime } }
      : null;
    serviceStatusMap[selectedRunnerId] = await fetchServiceStatus(selectedRunnerId);
  }

  async function refreshAllStatuses() {
    const runtime = await fetchRunnerStatusAll();
    if (snapshot) {
      snapshot = { ...snapshot, runtime };
    }
    serviceStatusMap = await fetchServiceStatusAll();
  }

  async function refreshLogs() {
    if (!selectedRunnerId) return;
    logSources = await listLogSources(selectedRunnerId);
    if (!selectedLogSource && logSources.length) {
      selectedLogSource = logSources[0].id;
    }
    if (selectedLogSource) {
      logLines = await tailLogs(selectedRunnerId, selectedLogSource, 200);
    }
  }

  function applySettingsSnapshot(snapshot: SettingsSnapshot) {
    settingsSnapshot = snapshot;
    autoUpdatesEnabled = snapshot.settings.auto_updates_enabled;
    autoCheckOnLaunch = snapshot.settings.auto_check_updates_on_launch;
    adoptionDefault = snapshot.settings.adoption_default;
  }

  async function loadSettings() {
    settingsError = null;
    try {
      const snapshot = await getSettings();
      applySettingsSnapshot(snapshot);
    } catch (error) {
      settingsError = `${error}`;
    } finally {
      settingsLoaded = true;
    }
  }

  async function persistSettings(patch: {
    auto_updates_enabled?: boolean;
    auto_check_updates_on_launch?: boolean;
    adoption_default?: AdoptionDefault;
  }) {
    settingsError = null;
    settingsBusy = true;
    try {
      const snapshot = await updateSettings(patch);
      applySettingsSnapshot(snapshot);
      if (!snapshot.settings.auto_updates_enabled) {
        pendingUpdate = null;
        updateError = null;
      }
    } catch (error) {
      settingsError = `${error}`;
      if (settingsSnapshot) {
        applySettingsSnapshot(settingsSnapshot);
      }
    } finally {
      settingsBusy = false;
    }
  }

  async function handleSelectRunner(runnerId: string) {
    selectedRunnerId = runnerId;
    await selectRunner(runnerId);
    await refreshSelectedStatus();
    await refreshLogs();
  }

  async function handleSavePat() {
    errorMessage = null;
    isBusy = true;
    try {
      await savePat(patAlias, patInput);
      patValid = await checkPat(patAlias);
      if (patValid) {
        await setDefaultPatAlias(patAlias);
        wizardStep = 2;
      }
    } catch (error) {
      errorMessage = `${error}`;
    } finally {
      isBusy = false;
    }
  }

  async function handleClearPat() {
    await clearPat(patAlias);
    patValid = false;
    patInput = "";
  }

  async function handleConfigure() {
    errorMessage = null;
    isBusy = true;
    try {
      let runnerId = createdRunnerId;
      if (!runnerId) {
        runnerId = await createRunnerProfile({
          display_name: displayName.trim() || runnerName.trim(),
          runner_name: runnerName.trim(),
          labels: labelsArray(),
          work_dir: workDir.trim(),
          scope: buildScope(),
          pat_alias: patAlias,
        });
        createdRunnerId = runnerId;
      }
      await downloadRunner(runnerId);
      await configureRunner({
        runnerId,
        scope: buildScope(),
        name: runnerName.trim(),
        labels: labelsArray(),
        workDir: workDir.trim(),
      });
      await refreshState();
      await handleSelectRunner(runnerId);
      wizardStep = 5;
      showCreate = false;
    } catch (error) {
      errorMessage = `${error}`;
    } finally {
      isBusy = false;
    }
  }

  async function handleStart() {
    if (!selectedRunnerId) return;
    errorMessage = null;
    isBusy = true;
    try {
      await startRunner(selectedRunnerId);
      await refreshSelectedStatus();
      await refreshLogs();
    } catch (error) {
      errorMessage = `${error}`;
    } finally {
      isBusy = false;
    }
  }

  async function handleStop() {
    if (!selectedRunnerId) return;
    errorMessage = null;
    isBusy = true;
    try {
      await stopRunner(selectedRunnerId);
      await refreshSelectedStatus();
    } catch (error) {
      errorMessage = `${error}`;
    } finally {
      isBusy = false;
    }
  }

  async function handleApplySettings() {
    if (!selectedRunnerId) return;
    const runner = selectedRunner();
    if (!runner || !runner.scope) {
      errorMessage = "Runner scope is missing; reconfigure in the setup flow.";
      return;
    }
    errorMessage = null;
    isBusy = true;
    try {
      await configureRunner({
        runnerId: selectedRunnerId,
        scope: runner.scope,
        name: runnerName.trim(),
        labels: labelsArray(),
        workDir: workDir.trim(),
      });
      await updateRunnerProfile(selectedRunnerId, {
        display_name: displayName.trim() || runnerName.trim(),
      });
      await refreshState();
      await refreshSelectedStatus();
    } catch (error) {
      errorMessage = `${error}`;
    } finally {
      isBusy = false;
    }
  }

  async function handleRunOnBoot(enabled: boolean) {
    if (!selectedRunnerId) return;
    const runner = selectedRunner();
    if (runner?.service.provider === "external") {
      errorMessage = "Runner is managed by an external service. Replace it before enabling run on boot.";
      return;
    }
    errorMessage = null;
    isBusy = true;
    try {
      if (enabled) {
        await installService(selectedRunnerId);
      }
      await setRunOnBoot(selectedRunnerId, enabled);
      await refreshState();
      await refreshSelectedStatus();
    } catch (error) {
      errorMessage = `${error}`;
    } finally {
      isBusy = false;
    }
  }

  async function handleCleanup(mode: "configonly" | "localdelete" | "unregisteranddelete") {
    if (!selectedRunnerId) return;
    const runner = selectedRunner();
    if (mode === "unregisteranddelete" && !runner?.scope) {
      errorMessage = "Runner scope is missing; cannot unregister from GitHub.";
      return;
    }
    const expected = runner?.display_name || runner?.runner_name || "delete";
    const prompt =
      mode === "configonly"
        ? "Remove this runner from RunnerBuddy only?"
        : mode === "localdelete"
          ? "Remove this runner and delete local files?"
          : "Unregister the runner on GitHub and delete local files?";
    if (mode === "configonly") {
      if (!confirm(prompt)) return;
    } else {
      if (!requireTypedConfirm(prompt, expected)) return;
    }
    errorMessage = null;
    isBusy = true;
    try {
      await deleteRunnerProfile(selectedRunnerId, mode);
      await refreshState();
      selectedRunnerId = snapshot?.config.selected_runner_id ?? null;
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
    } catch (error) {
      errorMessage = `${error}`;
    } finally {
      isScanning = false;
    }
  }

  async function handleImport(
    candidate: DiscoveryCandidate,
    options: { replace_service: boolean; move_install: boolean }
  ) {
    errorMessage = null;
    isBusy = true;
    try {
      const runnerId = await discoverImport(candidate.candidate_id, options);
      if (options.move_install) {
        const result = await discoverVerifyRunner(runnerId);
        if (!result.ok) {
          errorMessage = result.reason ?? "Verification failed.";
        }
      }
      await refreshState();
      await handleSelectRunner(runnerId);
      discoveryCandidates = discoveryCandidates.filter((item) => item.candidate_id !== candidate.candidate_id);
    } catch (error) {
      errorMessage = `${error}`;
    } finally {
      isBusy = false;
    }
  }

  async function handleReplaceService() {
    if (!selectedRunnerId) return;
    const runner = selectedRunner();
    const expected = runner?.display_name || runner?.runner_name || "replace";
    if (!requireTypedConfirm("Replace external service with RunnerBuddy?", expected)) {
      return;
    }
    errorMessage = null;
    isBusy = true;
    try {
      await discoverMigrateService(selectedRunnerId, "replacewithrunnerbuddy");
      await refreshState();
      await refreshSelectedStatus();
    } catch (error) {
      errorMessage = `${error}`;
    } finally {
      isBusy = false;
    }
  }

  async function handleMoveInstall() {
    if (!selectedRunnerId) return;
    errorMessage = null;
    isBusy = true;
    try {
      await discoverMoveInstall(selectedRunnerId);
      const result = await discoverVerifyRunner(selectedRunnerId);
      if (!result.ok) {
        errorMessage = result.reason ?? "Verification failed.";
      }
      await refreshState();
      await refreshSelectedStatus();
    } catch (error) {
      errorMessage = `${error}`;
    } finally {
      isBusy = false;
    }
  }

  async function handleVerifyInstall() {
    if (!selectedRunnerId) return;
    errorMessage = null;
    isBusy = true;
    try {
      const result = await discoverVerifyRunner(selectedRunnerId);
      if (!result.ok) {
        errorMessage = result.reason ?? "Verification failed.";
      }
      await refreshState();
      await refreshSelectedStatus();
    } catch (error) {
      errorMessage = `${error}`;
    } finally {
      isBusy = false;
    }
  }

  async function handleDeleteOriginalInstall() {
    if (!selectedRunnerId) return;
    const runner = selectedRunner();
    const expected = runner?.display_name || runner?.runner_name || "delete";
    if (!requireTypedConfirm("Delete the original runner install?", expected)) {
      return;
    }
    errorMessage = null;
    isBusy = true;
    try {
      await discoverDeleteOriginalInstall(selectedRunnerId);
      await refreshState();
    } catch (error) {
      errorMessage = `${error}`;
    } finally {
      isBusy = false;
    }
  }

  async function handleRemoveExternalArtifacts() {
    if (!selectedRunnerId) return;
    const runner = selectedRunner();
    const expected = runner?.display_name || runner?.runner_name || "remove";
    if (!requireTypedConfirm("Remove external service artifacts?", expected)) {
      return;
    }
    errorMessage = null;
    isBusy = true;
    try {
      await discoverRemoveExternalArtifacts(selectedRunnerId);
      await refreshState();
    } catch (error) {
      errorMessage = `${error}`;
    } finally {
      isBusy = false;
    }
  }

  async function handleAutoUpdatesToggle(enabled: boolean) {
    autoUpdatesEnabled = enabled;
    if (!enabled) {
      autoCheckOnLaunch = false;
    }
    await persistSettings({
      auto_updates_enabled: enabled,
      auto_check_updates_on_launch: enabled ? autoCheckOnLaunch : false,
    });
  }

  async function handleAutoCheckToggle(enabled: boolean) {
    if (!autoUpdatesEnabled) return;
    autoCheckOnLaunch = enabled;
    await persistSettings({ auto_check_updates_on_launch: enabled });
  }

  async function handleAdoptionDefaultChange(value: AdoptionDefault) {
    adoptionDefault = value;
    await persistSettings({ adoption_default: value });
  }

  async function handleRerunOnboarding() {
    if (
      !confirm(
        "Re-open onboarding? Existing runners are unchanged unless you choose actions."
      )
    ) {
      return;
    }
    settingsError = null;
    settingsBusy = true;
    try {
      const snapshot = await resetOnboarding();
      applySettingsSnapshot(snapshot);
      await goto("/onboarding");
    } catch (error) {
      settingsError = `${error}`;
    } finally {
      settingsBusy = false;
    }
  }

  onMount(() => {
    let unlistenProgress: (() => void) | null = null;
    let unlistenStatus: (() => void) | null = null;
    let cancelled = false;

    void (async () => {
      await refreshState();
      await refreshAllStatuses();
      await refreshLogs();
      await loadSettings();
      appVersion = await getVersion();
      if (settingsSnapshot && autoUpdatesEnabled && autoCheckOnLaunch) {
        void handleCheckUpdates({ silent: true });
      }
      if (cancelled) return;
      unlistenProgress = await listen<ProgressPayload>("progress", (event) => {
        progress = event.payload;
      });
      unlistenStatus = await listen<RunnerStatusPayload>("runner_status", (event) => {
        if (!snapshot) return;
        snapshot = {
          ...snapshot,
          runtime: {
            ...snapshot.runtime,
            [event.payload.runner_id]: {
              status: event.payload.status,
              pid: event.payload.pid,
              last_heartbeat: event.payload.last_heartbeat,
            },
          },
        };
      });
      statusTimer = window.setInterval(refreshSelectedStatus, 5000);
      logsTimer = window.setInterval(refreshLogs, 8000);
    })();

    return () => {
      cancelled = true;
      unlistenProgress?.();
      unlistenStatus?.();
      if (statusTimer) window.clearInterval(statusTimer);
      if (logsTimer) window.clearInterval(logsTimer);
    };
  });

  async function handleCheckUpdates(options?: { silent?: boolean }) {
    if (updateCheckBusy || updateInstallBusy) return;
    const silent = options?.silent ?? false;
    if (!silent) updateError = null;
    if (!settingsLoaded) {
      if (!silent) {
        updateError = "Settings are still loading.";
      }
      return;
    }
    if (!autoUpdatesEnabled) {
      if (!silent) {
        updateError = "Auto-updates are disabled in Settings.";
      }
      return;
    }
    updateCheckBusy = true;
    try {
      pendingUpdate = await check();
    } catch (error) {
      if (!silent) updateError = `${error}`;
    } finally {
      updateCheckBusy = false;
    }
  }

  async function handleInstallUpdate() {
    if (!pendingUpdate) return;
    if (!settingsLoaded) {
      updateError = "Settings are still loading.";
      return;
    }
    if (!autoUpdatesEnabled) {
      updateError = "Auto-updates are disabled in Settings.";
      return;
    }
    updateError = null;
    updateInstallBusy = true;
    try {
      await pendingUpdate.downloadAndInstall();
      await pendingUpdate.close();
      await relaunch();
    } catch (error) {
      updateError = `${error}`;
      try {
        await pendingUpdate.close();
      } catch {
        // ignore
      }
    } finally {
      updateInstallBusy = false;
    }
  }

  $effect(() => {
    const runner = selectedRunner();
    if (runner) {
      displayName = runner.display_name;
      runnerName = runner.runner_name;
      runnerLabels = runner.labels.join(", ");
      workDir = runner.work_dir;
      if (runner.scope) {
        scopeType = runner.scope.type;
        if (runner.scope.type === "repo") {
          scopeOwner = runner.scope.owner;
          scopeRepo = runner.scope.repo;
        }
        if (runner.scope.type === "org") {
          scopeOrg = runner.scope.org;
        }
        if (runner.scope.type === "enterprise") {
          scopeEnterprise = runner.scope.enterprise;
        }
      }
    }
  });
</script>

<main class="min-h-screen px-6 py-10 text-slate-100">
  <div class="mx-auto max-w-6xl space-y-8">
    <header class="flex flex-col gap-4 rounded-3xl px-8 py-8 glass-panel">
      <div class="flex flex-wrap items-center justify-between gap-4">
        <div>
          <p class="text-sm uppercase tracking-[0.2em] text-slate-400">RunnerBuddy</p>
          <h1 class="font-display text-3xl text-white">Multi-runner orchestration with calm control.</h1>
        </div>
        <div class="flex items-center gap-3">
          {#if selectedRunnerId}
            <span
              class={runnerRuntime(selectedRunnerId) === "running"
                ? "badge-live"
                : runnerRuntime(selectedRunnerId) === "idle"
                  ? "badge-idle"
                  : "badge-offline"}
            >
              {runnerRuntime(selectedRunnerId) ?? "offline"}
            </span>
          {/if}
        </div>
      </div>
      <p class="max-w-3xl text-sm text-slate-300">
        RunnerBuddy manages multiple GitHub Actions runners, discovers existing installs, and lets you
        unify services under one dashboard.
      </p>
    </header>

    {#if errorMessage}
      <div class="rounded-2xl border border-red-400/30 bg-red-500/10 px-4 py-3 text-sm text-red-100">
        {errorMessage}
      </div>
    {/if}

    <div class="grid gap-6 lg:grid-cols-[280px_1fr]">
      <aside class="space-y-6">
        <div class="rounded-2xl px-5 py-6 glass-panel">
          <div class="flex items-center justify-between">
            <h2 class="text-sm uppercase tracking-[0.2em] text-slate-400">Runners</h2>
            <button
              class="rounded-full border border-slate-400/30 px-3 py-1 text-xs text-slate-200"
              onclick={() => (showCreate = !showCreate)}
            >
              {showCreate ? "Close" : "Add"}
            </button>
          </div>
          <div class="mt-4 space-y-3">
            {#if snapshot?.config.runners.length}
              {#each snapshot.config.runners as runner}
                <button
                  class={`w-full rounded-xl border px-3 py-3 text-left text-sm transition ${
                    selectedRunnerId === runner.runner_id
                      ? "border-tide-400/60 bg-tide-500/15 text-white"
                      : "border-slate-500/40 text-slate-200 hover:border-slate-300/60"
                  }`}
                  onclick={() => handleSelectRunner(runner.runner_id)}
                >
                  <div class="flex items-center justify-between">
                    <span class="font-semibold">{runner.display_name}</span>
                    <span
                      class={`h-2 w-2 rounded-full ${
                        snapshot?.runtime[runner.runner_id]?.status === "running"
                          ? "bg-emerald-400"
                          : snapshot?.runtime[runner.runner_id]?.status === "idle"
                            ? "bg-amber-300"
                            : "bg-slate-500"
                      }`}
                    ></span>
                  </div>
                  <p class="mt-1 text-xs text-slate-400">
                    {runner.install.mode} · {runner.service.provider} · {runner.install.install_path}
                  </p>
                </button>
              {/each}
            {:else}
              <p class="text-sm text-slate-400">No runners yet.</p>
            {/if}
          </div>
          <button
            class="mt-5 w-full rounded-xl border border-slate-400/30 px-4 py-2 text-sm font-semibold text-slate-100"
            onclick={handleScan}
            disabled={isScanning}
          >
            {isScanning ? "Scanning..." : "Scan this machine"}
          </button>
        </div>

        <div class="rounded-2xl px-5 py-6 glass-panel">
          <h2 class="text-sm uppercase tracking-[0.2em] text-slate-400">Selection</h2>
          <div class="mt-4 space-y-3 text-sm">
            <div class="flex items-center justify-between">
              <span class="text-slate-300">Status</span>
              <span class="font-semibold text-white">{runnerRuntime(selectedRunnerId) ?? "-"}</span>
            </div>
            <div class="flex items-center justify-between">
              <span class="text-slate-300">Service</span>
              <span class="font-semibold text-white">
                {selectedRunnerId && serviceStatusMap[selectedRunnerId]?.running ? "Running" : "Stopped"}
              </span>
            </div>
          </div>
        </div>

        <div class="rounded-2xl px-5 py-6 glass-panel">
          <div class="flex items-center justify-between">
            <h2 class="text-sm uppercase tracking-[0.2em] text-slate-400">Updates</h2>
            <span class="text-xs text-slate-400">{appVersion ? `v${appVersion}` : ""}</span>
          </div>
          {#if updateError}
            <p class="mt-3 text-sm text-red-200">{updateError}</p>
          {/if}
          <div class="mt-4 space-y-3 text-sm">
            {#if !autoUpdatesEnabled}
              <p class="text-slate-400">Auto-updates are disabled.</p>
              <button
                class="w-full rounded-xl border border-slate-400/30 px-4 py-2 text-sm font-semibold text-slate-100 disabled:cursor-not-allowed disabled:opacity-60"
                disabled
              >
                Enable updates in Settings
              </button>
            {:else if pendingUpdate}
              <p class="text-slate-200">
                Update available: <span class="font-semibold text-white">{pendingUpdate.version}</span>
              </p>
              <button
                class="w-full rounded-xl bg-tide-500 px-4 py-2 text-sm font-semibold text-white disabled:cursor-not-allowed disabled:opacity-60"
                onclick={handleInstallUpdate}
                disabled={updateInstallBusy || isBusy}
              >
                {updateInstallBusy ? "Installing..." : "Install & relaunch"}
              </button>
            {:else}
              <p class="text-slate-400">No update loaded.</p>
              <button
                class="w-full rounded-xl border border-slate-400/30 px-4 py-2 text-sm font-semibold text-slate-100 disabled:cursor-not-allowed disabled:opacity-60"
                onclick={() => handleCheckUpdates()}
                disabled={updateCheckBusy || isBusy || !autoUpdatesEnabled || !settingsLoaded}
              >
                {updateCheckBusy ? "Checking..." : "Check for updates"}
              </button>
            {/if}
          </div>
        </div>

        <div class="rounded-2xl px-5 py-6 glass-panel">
          <div class="flex items-center justify-between">
            <h2 class="text-sm uppercase tracking-[0.2em] text-slate-400">Settings</h2>
          </div>
          {#if settingsError}
            <p class="mt-3 text-sm text-red-200">{settingsError}</p>
          {/if}
          {#if !settingsLoaded}
            <p class="mt-4 text-sm text-slate-400">Loading settings...</p>
          {:else}
            <div class="mt-4 space-y-3 text-sm">
              <label class="flex items-center justify-between rounded-xl border border-slate-500/40 px-4 py-3">
                <span class="text-slate-200">Enable auto-updates</span>
                <input
                  type="checkbox"
                  class="rounded border-slate-500 bg-transparent text-tide-500 focus:ring-tide-500"
                  checked={autoUpdatesEnabled}
                  onchange={(event) =>
                    handleAutoUpdatesToggle((event.target as HTMLInputElement).checked)
                  }
                  disabled={settingsBusy}
                />
              </label>
              <label
                class={`flex items-center justify-between rounded-xl border px-4 py-3 ${
                  autoUpdatesEnabled ? "border-slate-500/40" : "border-slate-600/30 opacity-60"
                }`}
              >
                <span class="text-slate-200">Auto-check on launch</span>
                <input
                  type="checkbox"
                  class="rounded border-slate-500 bg-transparent text-tide-500 focus:ring-tide-500"
                  checked={autoCheckOnLaunch}
                  onchange={(event) =>
                    handleAutoCheckToggle((event.target as HTMLInputElement).checked)
                  }
                  disabled={!autoUpdatesEnabled || settingsBusy}
                />
              </label>

              <div class="rounded-xl border border-slate-500/40 px-4 py-3 text-sm">
                <p class="text-xs uppercase tracking-[0.2em] text-slate-400">Adoption default</p>
                <div class="mt-3 space-y-2 text-xs text-slate-300">
                  <label class="flex items-center gap-2">
                    <input
                      type="radio"
                      name="adoption-default"
                      checked={adoptionDefault === "adopt"}
                      onchange={() => handleAdoptionDefaultChange("adopt")}
                      disabled={settingsBusy}
                    />
                    Adopt in place
                  </label>
                  <label class="flex items-center gap-2">
                    <input
                      type="radio"
                      name="adoption-default"
                      checked={adoptionDefault === "move_verify_delete"}
                      onchange={() => handleAdoptionDefaultChange("move_verify_delete")}
                      disabled={settingsBusy}
                    />
                    Move, verify, optional delete
                  </label>
                </div>
              </div>
            </div>

            <button
              class="mt-4 w-full rounded-xl border border-slate-400/30 px-4 py-2 text-sm font-semibold text-slate-100"
              onclick={handleRerunOnboarding}
              disabled={settingsBusy}
            >
              Re-run onboarding
            </button>
          {/if}
        </div>
      </aside>

      <section class="space-y-6">
        {#if showCreate}
          <div class="rounded-2xl px-6 py-6 glass-panel">
            <div class="flex flex-wrap items-center justify-between gap-4">
              <div>
                <h2 class="text-xl font-display text-white">New runner</h2>
                <p class="text-sm text-slate-300">
                  Create a managed runner profile and guide it online.
                </p>
              </div>
              <button
                class="rounded-xl border border-slate-500/40 px-4 py-2 text-sm font-semibold text-slate-200"
                onclick={() => (showCreate = false)}
              >
                Close
              </button>
            </div>

            <div class="mt-6 grid gap-4 lg:grid-cols-[200px_1fr]">
              <div class="space-y-3">
                {#each stepTitles as title, index}
                  <div
                    class={`rounded-xl border px-3 py-2 text-sm ${
                      wizardStep === index + 1
                        ? "border-tide-400/60 bg-tide-500/10 text-white"
                        : wizardStep > index + 1
                          ? "border-emerald-400/60 bg-emerald-500/10 text-emerald-100"
                          : "border-slate-500/40 text-slate-300"
                    }`}
                  >
                    {index + 1}. {title}
                  </div>
                {/each}
              </div>

              <div class="space-y-6">
                {#if wizardStep === 1}
                  <div class="space-y-4">
                    <h3 class="text-lg font-semibold">Save personal access token</h3>
                    <p class="text-sm text-slate-300">
                      Tokens are stored in the OS credential store. Choose a label so you can reuse it.
                    </p>
                    <input
                      type="text"
                      class="w-full rounded-xl border border-slate-500/40 bg-slate-950/40 px-4 py-2 text-sm text-white"
                      placeholder="Alias (default)"
                      bind:value={patAlias}
                    />
                    <input
                      type="password"
                      class="w-full rounded-xl border border-slate-500/40 bg-slate-950/40 px-4 py-2 text-sm text-white"
                      placeholder="ghp_..."
                      bind:value={patInput}
                    />
                    <div class="flex flex-wrap gap-3">
                      <button
                        class="rounded-xl bg-tide-500 px-4 py-2 text-sm font-semibold text-white"
                        onclick={handleSavePat}
                        disabled={isBusy || !patInput || !patAlias.trim()}
                      >
                        Save & validate
                      </button>
                      <button
                        class="rounded-xl border border-slate-500/40 px-4 py-2 text-sm font-semibold text-slate-200"
                        onclick={handleClearPat}
                        disabled={isBusy || !patAlias.trim()}
                      >
                        Clear token
                      </button>
                    </div>
                    {#if patValid}
                      <p class="text-sm text-emerald-200">Token validated. Continue.</p>
                    {/if}
                  </div>
                {/if}

                {#if wizardStep === 2}
                  <div class="space-y-4">
                    <h3 class="text-lg font-semibold">Choose scope</h3>
                    <div class="grid gap-3 sm:grid-cols-3">
                      <button
                        class={`rounded-xl border px-4 py-2 text-sm ${
                          scopeType === "repo"
                            ? "border-tide-400 bg-tide-500/20 text-white"
                            : "border-slate-500/40 text-slate-200"
                        }`}
                        onclick={() => (scopeType = "repo")}
                      >
                        Repo
                      </button>
                      <button
                        class={`rounded-xl border px-4 py-2 text-sm ${
                          scopeType === "org"
                            ? "border-tide-400 bg-tide-500/20 text-white"
                            : "border-slate-500/40 text-slate-200"
                        }`}
                        onclick={() => (scopeType = "org")}
                      >
                        Org
                      </button>
                      <button
                        class={`rounded-xl border px-4 py-2 text-sm ${
                          scopeType === "enterprise"
                            ? "border-tide-400 bg-tide-500/20 text-white"
                            : "border-slate-500/40 text-slate-200"
                        }`}
                        onclick={() => (scopeType = "enterprise")}
                      >
                        Enterprise
                      </button>
                    </div>

                    {#if scopeType === "repo"}
                      <div class="grid gap-3 sm:grid-cols-2">
                        <input
                          class="w-full rounded-xl border border-slate-500/40 bg-slate-950/40 px-4 py-2 text-sm text-white"
                          placeholder="owner"
                          bind:value={scopeOwner}
                        />
                        <input
                          class="w-full rounded-xl border border-slate-500/40 bg-slate-950/40 px-4 py-2 text-sm text-white"
                          placeholder="repo"
                          bind:value={scopeRepo}
                        />
                      </div>
                    {/if}

                    {#if scopeType === "org"}
                      <input
                        class="w-full rounded-xl border border-slate-500/40 bg-slate-950/40 px-4 py-2 text-sm text-white"
                        placeholder="org name"
                        bind:value={scopeOrg}
                      />
                    {/if}

                    {#if scopeType === "enterprise"}
                      <input
                        class="w-full rounded-xl border border-slate-500/40 bg-slate-950/40 px-4 py-2 text-sm text-white"
                        placeholder="enterprise slug"
                        bind:value={scopeEnterprise}
                      />
                    {/if}

                    <button
                      class="rounded-xl bg-tide-500 px-4 py-2 text-sm font-semibold text-white"
                      onclick={() => (wizardStep = 3)}
                      disabled={
                        (scopeType === "repo" && (!scopeOwner.trim() || !scopeRepo.trim())) ||
                        (scopeType === "org" && !scopeOrg.trim()) ||
                        (scopeType === "enterprise" && !scopeEnterprise.trim())
                      }
                    >
                      Continue
                    </button>
                  </div>
                {/if}

                {#if wizardStep === 3}
                  <div class="space-y-4">
                    <h3 class="text-lg font-semibold">Runner settings</h3>
                    <div class="grid gap-3 sm:grid-cols-2">
                      <input
                        class="w-full rounded-xl border border-slate-500/40 bg-slate-950/40 px-4 py-2 text-sm text-white"
                        placeholder="Display name"
                        bind:value={displayName}
                      />
                      <input
                        class="w-full rounded-xl border border-slate-500/40 bg-slate-950/40 px-4 py-2 text-sm text-white"
                        placeholder="Runner name"
                        bind:value={runnerName}
                      />
                    </div>
                    <input
                      class="w-full rounded-xl border border-slate-500/40 bg-slate-950/40 px-4 py-2 text-sm text-white"
                      placeholder="Labels (comma separated)"
                      bind:value={runnerLabels}
                    />
                    <input
                      class="w-full rounded-xl border border-slate-500/40 bg-slate-950/40 px-4 py-2 text-sm text-white"
                      placeholder="Work directory"
                      bind:value={workDir}
                    />
                    <div class="flex flex-wrap gap-3">
                      <button
                        class="rounded-xl border border-slate-500/40 px-4 py-2 text-sm font-semibold text-slate-200"
                        onclick={() => (wizardStep = 2)}
                      >
                        Back
                      </button>
                      <button
                        class="rounded-xl bg-tide-500 px-4 py-2 text-sm font-semibold text-white"
                        onclick={() => (wizardStep = 4)}
                        disabled={!runnerName.trim() || !workDir.trim()}
                      >
                        Continue
                      </button>
                    </div>
                  </div>
                {/if}

                {#if wizardStep === 4}
                  <div class="space-y-4">
                    <h3 class="text-lg font-semibold">Download & configure</h3>
                    <p class="text-sm text-slate-300">
                      RunnerBuddy downloads the latest runner, extracts it, and registers with GitHub.
                    </p>
                    {#if progress && (!createdRunnerId || progress.runner_id === createdRunnerId)}
                      <div class="rounded-xl border border-slate-500/40 px-4 py-3 text-sm">
                        <p class="text-slate-200">
                          {progress.phase} — {progress.percent}%
                        </p>
                        <div class="mt-2 h-2 w-full overflow-hidden rounded-full bg-slate-700">
                          <div class="h-2 bg-tide-500" style={`width: ${progress.percent}%`}></div>
                        </div>
                      </div>
                    {/if}
                    <button
                      class="rounded-xl bg-tide-500 px-4 py-2 text-sm font-semibold text-white"
                      onclick={handleConfigure}
                      disabled={isBusy}
                    >
                      Download & configure
                    </button>
                  </div>
                {/if}

                {#if wizardStep === 5}
                  <div class="space-y-4">
                    <h3 class="text-lg font-semibold">Runner controls</h3>
                    <p class="text-sm text-slate-300">
                      Start the runner, then enable run-on-boot if you want launchd/systemd to manage it.
                    </p>
                    <div class="flex flex-wrap gap-3">
                      <button
                        class="rounded-xl bg-tide-500 px-4 py-2 text-sm font-semibold text-white"
                        onclick={handleStart}
                        disabled={isBusy}
                      >
                        Start runner
                      </button>
                      <button
                        class="rounded-xl border border-slate-500/40 px-4 py-2 text-sm font-semibold text-slate-200"
                        onclick={handleStop}
                        disabled={isBusy}
                      >
                        Stop runner
                      </button>
                    </div>
                  </div>
                {/if}
              </div>
            </div>
          </div>
        {/if}

        {#if discoveryCandidates.length}
          <div class="rounded-2xl px-6 py-6 glass-panel">
            <div class="flex flex-wrap items-center justify-between gap-4">
              <div>
                <h2 class="text-xl font-display text-white">Discovered runners</h2>
                <p class="text-sm text-slate-300">Adopt existing installs or replace their services.</p>
              </div>
            </div>
            <div class="mt-6 space-y-4">
              {#each discoveryCandidates as candidate}
                <div class="rounded-xl border border-slate-500/40 bg-slate-950/40 px-4 py-4">
                  <div class="flex flex-wrap items-center justify-between gap-4">
                    <div>
                      <p class="text-sm font-semibold text-white">
                        {candidate.runner_name ?? "Unknown runner"}
                      </p>
                      <p class="text-xs text-slate-400">{candidate.install_path}</p>
                      <p class="text-xs text-slate-500">Scope: {scopeLabel(candidate.scope ?? null)}</p>
                      {#if candidate.service_present}
                        <p class="text-xs text-slate-500">
                          Service: {candidate.service_id ?? "external"}{candidate.service_path ? ` · ${candidate.service_path}` : ""}
                        </p>
                      {/if}
                    </div>
                    <span class="text-xs text-slate-300">
                      {candidate.service_present ? "Service detected" : "No service"}
                    </span>
                  </div>
                  <div class="mt-3 flex flex-wrap gap-3">
                    <button
                      class="rounded-lg bg-tide-500 px-3 py-1 text-xs font-semibold text-white"
                      onclick={() => handleImport(candidate, { replace_service: false, move_install: false })}
                      disabled={isBusy}
                    >
                      Adopt
                    </button>
                    {#if candidate.service_present}
                      <button
                        class="rounded-lg border border-slate-400/50 px-3 py-1 text-xs font-semibold text-slate-200"
                        onclick={() => handleImport(candidate, { replace_service: true, move_install: false })}
                        disabled={isBusy}
                      >
                        Adopt + replace service
                      </button>
                    {/if}
                    <button
                      class="rounded-lg border border-slate-400/50 px-3 py-1 text-xs font-semibold text-slate-200"
                      onclick={() => handleImport(candidate, { replace_service: false, move_install: true })}
                      disabled={isBusy}
                    >
                      Adopt + move (verify)
                    </button>
                  </div>
                </div>
              {/each}
            </div>
          </div>
        {/if}

        {#if selectedRunner()}
          <div class="rounded-2xl px-6 py-6 glass-panel">
            <div class="flex flex-wrap items-center justify-between gap-4">
              <div>
                <h2 class="text-xl font-display text-white">Runner controls</h2>
                <p class="text-sm text-slate-300">Start, stop, and manage services for this runner.</p>
              </div>
              <div class="flex items-center gap-2">
                <span class="text-xs text-slate-400">{selectedRunner()?.install.mode}</span>
                {#if selectedRunner()?.install.mode === "adopted"}
                  <button
                    class="rounded-xl border border-slate-400/40 px-3 py-1 text-xs text-slate-200"
                    onclick={handleMoveInstall}
                    disabled={isBusy || selectedRunner()?.service.provider === "external"}
                  >
                    Move + verify
                  </button>
                {/if}
              </div>
            </div>

            <div class="mt-6 grid gap-4 lg:grid-cols-[200px_1fr]">
              <div class="space-y-3">
                <div class="rounded-xl border border-slate-500/40 px-3 py-2 text-sm text-slate-200">
                  Status: <span class="font-semibold text-white">{runnerRuntime(selectedRunnerId) ?? "offline"}</span>
                </div>
                <div class="rounded-xl border border-slate-500/40 px-3 py-2 text-sm text-slate-200">
                  Service: <span class="font-semibold text-white">
                    {selectedRunnerId && serviceStatusMap[selectedRunnerId]?.running ? "Running" : "Stopped"}
                  </span>
                </div>
                <div class="rounded-xl border border-slate-500/40 px-3 py-2 text-sm text-slate-200">
                  Provider: <span class="font-semibold text-white">{selectedRunner()?.service.provider}</span>
                </div>
              </div>

              <div class="space-y-4">
                <div class="flex flex-wrap gap-3">
                  <button
                    class="rounded-xl bg-tide-500 px-4 py-2 text-sm font-semibold text-white"
                    onclick={handleStart}
                    disabled={isBusy}
                  >
                    Start runner
                  </button>
                  <button
                    class="rounded-xl border border-slate-500/40 px-4 py-2 text-sm font-semibold text-slate-200"
                    onclick={handleStop}
                    disabled={isBusy}
                  >
                    Stop runner
                  </button>
                </div>

                {#if selectedRunner()?.service.provider !== "external"}
                  <div class="flex items-center justify-between rounded-xl border border-slate-500/40 px-4 py-3 text-sm">
                    <span class="text-slate-300">Run on boot</span>
                    <label class="inline-flex items-center gap-2">
                      <input
                        type="checkbox"
                        class="rounded border-slate-500 bg-transparent text-tide-500 focus:ring-tide-500"
                        checked={selectedRunner()?.service.run_on_boot}
                        onchange={(event) => handleRunOnBoot((event.target as HTMLInputElement).checked)}
                        disabled={isBusy}
                      />
                      <span class="text-xs text-slate-300">
                        {selectedRunner()?.service.run_on_boot ? "On" : "Off"}
                      </span>
                    </label>
                  </div>
                {:else}
                  <div class="rounded-xl border border-amber-400/40 bg-amber-500/10 px-4 py-3 text-sm text-amber-100 space-y-2">
                    <p class="font-semibold">Managed by an external service</p>
                    <p class="text-xs text-amber-100/80">
                      External id: {selectedRunner()?.service.external_id ?? "unknown"}
                    </p>
                    <p class="text-xs text-amber-100/80">
                      External path: {selectedRunner()?.service.external_path ?? "unknown"}
                    </p>
                    <p class="text-xs text-amber-100/80">
                      Replace will disable/unload the external service and install a RunnerBuddy-managed service. Undo
                      by re-enabling the external service using the saved id/path.
                    </p>
                    <div class="flex flex-wrap gap-2">
                      <button
                        class="rounded-lg bg-amber-400/20 px-3 py-1 text-xs font-semibold text-amber-100"
                        onclick={handleReplaceService}
                        disabled={isBusy}
                      >
                        Replace with RunnerBuddy
                      </button>
                      <button
                        class="rounded-lg border border-amber-300/40 px-3 py-1 text-xs font-semibold text-amber-100"
                        onclick={handleRemoveExternalArtifacts}
                        disabled={isBusy}
                      >
                        Remove external artifacts
                      </button>
                    </div>
                  </div>
                {/if}

                {#if selectedRunner()?.service.external_restore}
                  <div class="rounded-xl border border-slate-500/40 bg-slate-950/40 px-4 py-3 text-xs text-slate-300">
                    External service saved for restore:
                    {selectedRunner()?.service.external_restore?.id ?? "unknown"} ·
                    {selectedRunner()?.service.external_restore?.path ?? "unknown"}
                  </div>
                {/if}

                {#if hasMigration(selectedRunner())}
                  <div class="rounded-xl border border-slate-500/40 bg-slate-950/40 px-4 py-3 text-sm text-slate-200">
                    <p class="font-semibold text-white">Migration</p>
                    <p class="text-xs text-slate-400">
                      Status: <span class="text-slate-200">{migrationStatus(selectedRunner())}</span>
                    </p>
                    {#if selectedRunner()?.install.adopted_from_path}
                      <p class="text-xs text-slate-400">
                        Original: {selectedRunner()?.install.adopted_from_path}
                      </p>
                    {/if}
                    <div class="mt-3 flex flex-wrap gap-2">
                      <button
                        class="rounded-lg border border-slate-400/50 px-3 py-1 text-xs font-semibold text-slate-200"
                        onclick={handleVerifyInstall}
                        disabled={isBusy || selectedRunner()?.service.provider === "external"}
                      >
                        Verify install
                      </button>
                      <button
                        class="rounded-lg border border-red-300/40 px-3 py-1 text-xs font-semibold text-red-100"
                        onclick={handleDeleteOriginalInstall}
                        disabled={isBusy || !canDeleteOriginal(selectedRunner())}
                      >
                        Delete original install
                      </button>
                    </div>
                  </div>
                {/if}
              </div>
            </div>
          </div>

          <div class="rounded-2xl px-6 py-6 glass-panel">
            <div class="flex flex-wrap items-center justify-between gap-4">
              <div>
                <h2 class="text-xl font-display text-white">Runner configuration</h2>
                <p class="text-sm text-slate-300">
                  Changing name or labels re-registers this runner with GitHub.
                </p>
              </div>
              <button
                class="rounded-xl bg-tide-500 px-4 py-2 text-sm font-semibold text-white"
                onclick={handleApplySettings}
                disabled={isBusy || !selectedRunner()?.scope}
              >
                Apply changes
              </button>
            </div>
            {#if !selectedRunner()?.scope}
              <p class="mt-3 text-xs text-amber-200">
                Scope is missing for this runner. Re-run configuration to set scope before applying changes.
              </p>
            {/if}
            <div class="mt-4 grid gap-3 sm:grid-cols-2">
              <input
                class="w-full rounded-xl border border-slate-500/40 bg-slate-950/40 px-4 py-2 text-sm text-white"
                placeholder="Display name"
                bind:value={displayName}
              />
              <input
                class="w-full rounded-xl border border-slate-500/40 bg-slate-950/40 px-4 py-2 text-sm text-white"
                placeholder="Runner name"
                bind:value={runnerName}
              />
            </div>
            <input
              class="mt-3 w-full rounded-xl border border-slate-500/40 bg-slate-950/40 px-4 py-2 text-sm text-white"
              placeholder="Labels (comma separated)"
              bind:value={runnerLabels}
            />
            <input
              class="mt-3 w-full rounded-xl border border-slate-500/40 bg-slate-950/40 px-4 py-2 text-sm text-white"
              placeholder="Work directory"
              bind:value={workDir}
            />
          </div>

          <div class="rounded-2xl px-6 py-6 glass-panel">
            <div class="flex flex-wrap items-center justify-between gap-4">
              <div>
                <h2 class="text-xl font-display text-white">Cleanup</h2>
                <p class="text-sm text-slate-300">Remove this runner safely.</p>
              </div>
            </div>
            <div class="mt-4 flex flex-wrap gap-3">
              <button
                class="rounded-xl border border-slate-500/40 px-4 py-2 text-sm font-semibold text-slate-200"
                onclick={() => handleCleanup("configonly")}
                disabled={isBusy}
              >
                Remove from RunnerBuddy
              </button>
              <button
                class="rounded-xl border border-red-300/40 px-4 py-2 text-sm font-semibold text-red-100"
                onclick={() => handleCleanup("localdelete")}
                disabled={isBusy}
              >
                Delete local files
              </button>
              <button
                class="rounded-xl bg-red-500/20 px-4 py-2 text-sm font-semibold text-red-100"
                onclick={() => handleCleanup("unregisteranddelete")}
                disabled={isBusy || !selectedRunner()?.scope}
              >
                Unregister & delete
              </button>
            </div>
            {#if !selectedRunner()?.scope}
              <p class="mt-3 text-xs text-amber-200">
                Scope is missing; unregistering from GitHub is unavailable.
              </p>
            {/if}
          </div>

          <div class="rounded-2xl px-6 py-6 glass-panel">
            <div class="flex flex-wrap items-center justify-between gap-4">
              <div>
                <h2 class="text-xl font-display text-white">Logs</h2>
                <p class="text-sm text-slate-300">Live view of app and runner diagnostics.</p>
              </div>
              <select
                class="rounded-xl border border-slate-500/40 bg-slate-950/40 px-3 py-2 text-sm"
                bind:value={selectedLogSource}
                onchange={refreshLogs}
              >
                {#each logSources as source}
                  <option value={source.id}>{source.label}</option>
                {/each}
              </select>
            </div>
            <div class="mt-4 max-h-80 overflow-auto rounded-xl border border-slate-500/30 bg-slate-950/40 p-3 font-mono text-xs text-slate-200">
              {#if logLines.length === 0}
                <p class="text-slate-400">No log lines yet.</p>
              {:else}
                {#each logLines as line}
                  <div>{line.line}</div>
                {/each}
              {/if}
            </div>
          </div>
        {:else}
          <div class="rounded-2xl px-6 py-10 glass-panel">
            <h2 class="text-xl font-display text-white">No runner selected</h2>
            <p class="mt-2 text-sm text-slate-300">
              Add a runner or import one from this machine to get started.
            </p>
          </div>
        {/if}
      </section>
    </div>
  </div>
</main>

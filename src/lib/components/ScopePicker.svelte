<script lang="ts">
  import type { GitHubOrgInfo, GitHubRepoInfo } from "$lib/api";

  type ScopeType = "repo" | "org" | "enterprise";

  export let scopeType: ScopeType;
  export let scopeOwner = "";
  export let scopeRepo = "";
  export let scopeOrg = "";
  export let scopeEnterprise = "";

  export let repoOptions: GitHubRepoInfo[] = [];
  export let repoFilter = "";
  export let repoAdminOnly = true;
  export let reposBusy = false;
  export let reposError: string | null = null;

  export let orgOptions: GitHubOrgInfo[] = [];
  export let orgFilter = "";
  export let orgsBusy = false;
  export let orgsError: string | null = null;

  export let isBusy = false;
  export let onScopeChange: (() => void | Promise<void>) | undefined;
  export let onMarkDirty: (() => void | Promise<void>) | undefined;
  export let onLoadRepos: (() => void | Promise<void>) | undefined;
  export let onLoadOrgs: (() => void | Promise<void>) | undefined;

  export let showTokenTools = false;
  export let scopeIsComplete: (() => boolean) | undefined = undefined;
  export let onOpenSettings: (() => void | Promise<void>) | undefined = undefined;
  export let onFetchToken: (() => void | Promise<void>) | undefined = undefined;
  export let onCopyToken: (() => void | Promise<void>) | undefined = undefined;
  export let tokenBusy = false;
  export let tokenError: string | null = null;
  export let registrationToken: string | null = null;
  export let registrationTokenExpiresAt: string | null = null;

  export let variant: "wizard" | "compact" = "wizard";

  const inputBg = variant === "wizard" ? "bg-slate-950/40" : "bg-slate-950/20";
  const panelBg = variant === "wizard" ? "bg-slate-950/40" : "bg-slate-950/20";
  const filterBg = variant === "wizard" ? "bg-slate-950/20" : "bg-slate-950/10";
  const listHeight = variant === "wizard" ? "max-h-56" : "max-h-48";

  function handleScopeTypeChange(next: ScopeType) {
    scopeType = next;
    onScopeChange?.();
    onMarkDirty?.();
  }

  function handleScopeInput() {
    onScopeChange?.();
    onMarkDirty?.();
  }

  function repoHasAdmin(repo: GitHubRepoInfo): boolean {
    return repo.permissions?.admin ?? false;
  }

  function filteredRepos(): GitHubRepoInfo[] {
    const query = repoFilter.trim().toLowerCase();
    return repoOptions.filter((repo) => {
      if (repoAdminOnly && !repoHasAdmin(repo)) return false;
      if (!query) return true;
      return (
        repo.name_with_owner.toLowerCase().includes(query) ||
        repo.owner.toLowerCase().includes(query) ||
        repo.repo.toLowerCase().includes(query)
      );
    });
  }

  function filteredOrgs(): GitHubOrgInfo[] {
    const query = orgFilter.trim().toLowerCase();
    return orgOptions.filter((org) => {
      if (!query) return true;
      return org.org.toLowerCase().includes(query);
    });
  }

  function selectRepo(repo: GitHubRepoInfo) {
    scopeOwner = repo.owner;
    scopeRepo = repo.repo;
    onScopeChange?.();
    onMarkDirty?.();
  }

  function selectOrg(org: GitHubOrgInfo) {
    scopeOrg = org.org;
    onScopeChange?.();
    onMarkDirty?.();
  }

  function isScopeCompleteValue() {
    return scopeIsComplete ? scopeIsComplete() : false;
  }
</script>

<div class="space-y-4">
  <div class="grid gap-3 sm:grid-cols-3">
    <button
      class={`rounded-xl border px-4 py-2 text-sm ${
        scopeType === "repo"
          ? "border-tide-400 bg-tide-500/20 text-white"
          : "border-slate-500/40 text-slate-200"
      }`}
      onclick={() => handleScopeTypeChange("repo")}
    >
      Repo
    </button>
    <button
      class={`rounded-xl border px-4 py-2 text-sm ${
        scopeType === "org"
          ? "border-tide-400 bg-tide-500/20 text-white"
          : "border-slate-500/40 text-slate-200"
      }`}
      onclick={() => handleScopeTypeChange("org")}
    >
      Org
    </button>
    <button
      class={`rounded-xl border px-4 py-2 text-sm ${
        scopeType === "enterprise"
          ? "border-tide-400 bg-tide-500/20 text-white"
          : "border-slate-500/40 text-slate-200"
      }`}
      onclick={() => handleScopeTypeChange("enterprise")}
    >
      Enterprise
    </button>
  </div>

  {#if scopeType === "repo"}
    <div class="grid gap-3 sm:grid-cols-2">
      <input
        class={`w-full rounded-xl border border-slate-500/40 ${inputBg} px-4 py-2 text-sm text-white`}
        placeholder="owner"
        bind:value={scopeOwner}
        oninput={handleScopeInput}
      />
      <input
        class={`w-full rounded-xl border border-slate-500/40 ${inputBg} px-4 py-2 text-sm text-white`}
        placeholder="repo"
        bind:value={scopeRepo}
        oninput={handleScopeInput}
      />
    </div>

    <div class={`rounded-xl border border-slate-500/40 ${panelBg} px-4 py-3`}>
      <div class="flex flex-wrap items-center justify-between gap-3">
        <p class="text-xs text-slate-300">Pick from your repos</p>
        <button
          class="rounded-lg border border-slate-400/40 px-3 py-1 text-xs font-semibold text-slate-200 disabled:cursor-not-allowed disabled:opacity-60"
          onclick={onLoadRepos}
          disabled={reposBusy || isBusy || !onLoadRepos}
        >
          {reposBusy ? "Loading..." : repoOptions.length ? "Refresh" : "Load repos"}
        </button>
      </div>
      {#if reposError}
        <p class="mt-2 text-xs text-red-200">{reposError}</p>
      {/if}
      {#if repoOptions.length}
        {@const visibleRepos = filteredRepos()}
        <div class="mt-3 flex flex-wrap items-center gap-3">
          <input
            class={`w-full rounded-lg border border-slate-500/40 ${filterBg} px-3 py-1 text-xs text-white sm:flex-1`}
            placeholder="Filter (owner/repo)"
            bind:value={repoFilter}
          />
          <label class="flex items-center gap-2 text-xs text-slate-300">
            <input type="checkbox" bind:checked={repoAdminOnly} />
            Admin only
          </label>
          <span class="text-xs text-slate-500">{visibleRepos.length} shown</span>
        </div>
        <div class={`mt-3 ${listHeight} overflow-auto rounded-lg border border-slate-500/40`}>
          {#each visibleRepos.slice(0, 200) as repo (repo.name_with_owner)}
            <button
              class="flex w-full items-center justify-between gap-3 border-b border-slate-500/30 px-3 py-2 text-left text-xs text-slate-100 hover:bg-slate-900/40"
              onclick={() => selectRepo(repo)}
            >
              <span class="min-w-0 flex-1 truncate">{repo.name_with_owner}</span>
              <span class="shrink-0 text-slate-400">
                {repo.private ? "private" : "public"}
                {repoHasAdmin(repo) ? " · admin" : ""}
              </span>
            </button>
          {/each}
          {#if visibleRepos.length > 200}
            <p class="px-3 py-2 text-xs text-slate-500">
              Showing first 200 results. Refine the filter to narrow.
            </p>
          {/if}
          {#if visibleRepos.length === 0}
            <p class="px-3 py-2 text-xs text-slate-500">No matches.</p>
          {/if}
        </div>
      {/if}
    </div>
  {/if}

  {#if scopeType === "org"}
    <input
      class={`w-full rounded-xl border border-slate-500/40 ${inputBg} px-4 py-2 text-sm text-white`}
      placeholder="org name"
      bind:value={scopeOrg}
      oninput={handleScopeInput}
    />

    <div class={`rounded-xl border border-slate-500/40 ${panelBg} px-4 py-3`}>
      <div class="flex flex-wrap items-center justify-between gap-3">
        <p class="text-xs text-slate-300">Pick from your orgs</p>
        <button
          class="rounded-lg border border-slate-400/40 px-3 py-1 text-xs font-semibold text-slate-200 disabled:cursor-not-allowed disabled:opacity-60"
          onclick={onLoadOrgs}
          disabled={orgsBusy || isBusy || !onLoadOrgs}
        >
          {orgsBusy ? "Loading..." : orgOptions.length ? "Refresh" : "Load orgs"}
        </button>
      </div>
      {#if orgsError}
        <p class="mt-2 text-xs text-red-200">{orgsError}</p>
      {/if}
      {#if orgOptions.length}
        {@const visibleOrgs = filteredOrgs()}
        <div class="mt-3 flex flex-wrap items-center gap-3">
          <input
            class={`w-full rounded-lg border border-slate-500/40 ${filterBg} px-3 py-1 text-xs text-white sm:flex-1`}
            placeholder="Filter orgs"
            bind:value={orgFilter}
          />
          <span class="text-xs text-slate-500">{visibleOrgs.length} shown</span>
        </div>
        <div class={`mt-3 ${listHeight} overflow-auto rounded-lg border border-slate-500/40`}>
          {#each visibleOrgs.slice(0, 200) as org (org.org)}
            <button
              class="flex w-full items-center justify-between gap-3 border-b border-slate-500/30 px-3 py-2 text-left text-xs text-slate-100 hover:bg-slate-900/40"
              onclick={() => selectOrg(org)}
            >
              <span class="min-w-0 flex-1 truncate">{org.org}</span>
              <span class="shrink-0 text-slate-400">{org.url}</span>
            </button>
          {/each}
          {#if visibleOrgs.length > 200}
            <p class="px-3 py-2 text-xs text-slate-500">
              Showing first 200 results. Refine the filter to narrow.
            </p>
          {/if}
          {#if visibleOrgs.length === 0}
            <p class="px-3 py-2 text-xs text-slate-500">No matches.</p>
          {/if}
        </div>
      {/if}
    </div>
  {/if}

  {#if scopeType === "enterprise"}
    <input
      class={`w-full rounded-xl border border-slate-500/40 ${inputBg} px-4 py-2 text-sm text-white`}
      placeholder="enterprise slug"
      bind:value={scopeEnterprise}
      oninput={handleScopeInput}
    />
  {/if}

  {#if showTokenTools}
    <div class={`rounded-xl border border-slate-500/40 ${panelBg} px-4 py-3`}>
      <p class="text-xs text-slate-300">
        RunnerBuddy fetches runner registration tokens automatically during setup, but you can
        open the GitHub runners page or fetch a token manually.
      </p>
      <div class="mt-3 flex flex-wrap gap-3">
        <button
          class="rounded-lg border border-slate-400/40 px-3 py-1 text-xs font-semibold text-slate-200 disabled:cursor-not-allowed disabled:opacity-60"
          onclick={onOpenSettings}
          disabled={!isScopeCompleteValue() || isBusy || !onOpenSettings}
        >
          Open self-hosted runners page
        </button>
        <button
          class="rounded-lg bg-tide-500 px-3 py-1 text-xs font-semibold text-white disabled:cursor-not-allowed disabled:opacity-60"
          onclick={onFetchToken}
          disabled={!isScopeCompleteValue() || tokenBusy || isBusy || !onFetchToken}
        >
          {tokenBusy ? "Fetching..." : "Fetch registration token"}
        </button>
      </div>
      {#if tokenError}
        <p class="mt-2 text-xs text-red-200">{tokenError}</p>
      {/if}
      {#if registrationToken}
        <div class="mt-3 space-y-2">
          <p class="text-xs text-slate-400">
            Token expires at {registrationTokenExpiresAt ?? "unknown"}
          </p>
          <div class="flex flex-wrap items-center gap-3">
            <input
              class={`w-full flex-1 rounded-lg border border-slate-500/40 ${filterBg} px-3 py-1 font-mono text-xs text-white`}
              readonly
              value={registrationToken}
            />
            <button
              class="rounded-lg border border-slate-400/40 px-3 py-1 text-xs font-semibold text-slate-200"
              onclick={onCopyToken}
            >
              Copy
            </button>
          </div>
        </div>
      {/if}
    </div>
  {/if}
</div>

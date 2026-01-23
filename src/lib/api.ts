import { invoke } from "@tauri-apps/api/core";

export type RunnerStatus = "offline" | "idle" | "running";

export type AdoptionDefault = "adopt" | "move_verify_delete";

export type RunnerScope =
  | { type: "repo"; owner: string; repo: string }
  | { type: "org"; org: string }
  | { type: "enterprise"; enterprise: string };

export interface GitHubRegistrationToken {
  token: string;
  expires_at: string;
}

export interface GitHubRepoPermissions {
  admin: boolean;
  push: boolean;
  pull: boolean;
}

export interface GitHubRepoInfo {
  owner: string;
  repo: string;
  name_with_owner: string;
  url: string;
  private: boolean;
  permissions?: GitHubRepoPermissions | null;
}

export interface GitHubOrgInfo {
  org: string;
  url: string;
}

export type InstallMode = "managed" | "adopted";

export type ServiceProvider = "runnerbuddy" | "external" | "unknown";

export type MigrationStatus = "none" | "moved" | "verified" | "failed";

export interface InstallConfig {
  mode: InstallMode;
  install_path: string;
  adopted_from_path?: string | null;
  migration_status?: MigrationStatus | null;
}

export interface RunnerServiceConfig {
  installed: boolean;
  run_on_boot: boolean;
  provider: ServiceProvider;
  external_id?: string | null;
  external_path?: string | null;
  external_restore?: {
    id?: string | null;
    path?: string | null;
  } | null;
}

export interface RunnerProfile {
  runner_id: string;
  display_name: string;
  scope: RunnerScope | null;
  runner_name: string;
  labels: string[];
  work_dir: string;
  install: InstallConfig;
  runner_version?: string | null;
  pat_alias: string;
  service: RunnerServiceConfig;
  created_at: string;
  last_seen_at?: string | null;
}

export interface OnboardingConfig {
  completed: boolean;
  completed_at?: string | null;
}

export interface SettingsConfig {
  auto_updates_enabled: boolean;
  auto_check_updates_on_launch: boolean;
  adoption_default: AdoptionDefault;
}

export interface Config {
  schema_version: number;
  selected_runner_id?: string | null;
  pat_default_alias: string;
  onboarding: OnboardingConfig;
  settings: SettingsConfig;
  runners: RunnerProfile[];
}

export interface RuntimeState {
  status: RunnerStatus;
  pid?: number | null;
  last_heartbeat?: number | null;
  last_error?: string | null;
}

export interface AppSnapshot {
  config: Config;
  runtime: Record<string, RuntimeState>;
}

export interface RunnerDefaults {
  runner_id: string;
  display_name: string;
  runner_name: string;
  labels: string[];
  work_dir: string;
}

export interface SettingsSnapshot {
  onboarding: OnboardingConfig;
  settings: SettingsConfig;
}

export interface ServiceStatus {
  installed: boolean;
  running: boolean;
  enabled: boolean;
}

export interface LogSource {
  id: string;
  label: string;
  path: string;
}

export interface LogLine {
  line: string;
}

export interface ProgressPayload {
  runner_id: string;
  phase: string;
  percent: number;
}

export interface DiscoveryCandidate {
  candidate_id: string;
  install_path: string;
  runner_name?: string | null;
  labels: string[];
  scope?: RunnerScope | null;
  work_dir?: string | null;
  service_present: boolean;
  service_id?: string | null;
  service_path?: string | null;
  last_log_time?: string | null;
}

export type RunnerDeleteMode = "configonly" | "localdelete" | "unregisteranddelete";

export type ServiceMigrationStrategy = "replacewithrunnerbuddy";

const call = <T>(command: string, args?: Record<string, unknown>): Promise<T> =>
  invoke<T>(command, args);

export const runnersList = (): Promise<AppSnapshot> => call("runners_list");

export const runnersDefaultProfile = (): Promise<RunnerDefaults> =>
  call("runners_default_profile");

export const getSettings = (): Promise<SettingsSnapshot> => call("settings_get");

export const updateSettings = (
  patch: Partial<SettingsConfig>
): Promise<SettingsSnapshot> => call("settings_update", { patch });

export const completeOnboarding = (): Promise<SettingsSnapshot> =>
  call("onboarding_complete");

export const resetOnboarding = (): Promise<SettingsSnapshot> =>
  call("onboarding_reset");

export const createRunnerProfile = (params: {
  runner_id?: string;
  display_name?: string;
  runner_name?: string;
  labels?: string[];
  work_dir?: string;
  scope?: RunnerScope | null;
  pat_alias?: string;
}): Promise<string> => call("runners_create_profile", { input: params });

export const updateRunnerProfile = (
  runnerId: string,
  patch: {
    display_name?: string;
    runner_name?: string;
    labels?: string[];
    work_dir?: string;
    scope?: RunnerScope | null;
    pat_alias?: string;
  }
): Promise<RunnerProfile> => call("runners_update_profile", { runnerId, patch });

export const deleteRunnerProfile = (
  runnerId: string,
  mode: RunnerDeleteMode
): Promise<void> => call("runners_delete_profile", { runnerId, mode });

export const selectRunner = (runnerId: string | null): Promise<void> =>
  call("runners_select", { runnerId });

export const savePat = (alias: string, pat: string): Promise<void> =>
  call("auth_save_pat", { alias, pat });

export const importGhToken = (alias: string): Promise<void> =>
  call("auth_import_gh_token", { alias });

export const clearPat = (alias: string): Promise<void> =>
  call("auth_clear_pat", { alias });

export const checkPat = (alias: string): Promise<boolean> =>
  call("auth_check_pat", { alias });

export const setDefaultPatAlias = (alias: string): Promise<void> =>
  call("auth_set_default_alias", { alias });

export const githubGetRegistrationToken = (
  scope: RunnerScope,
  alias: string
): Promise<GitHubRegistrationToken> =>
  call("github_get_registration_token", { scope, alias });

export const githubListRepos = (alias: string): Promise<GitHubRepoInfo[]> =>
  call("github_list_repos", { alias });

export const githubListOrgs = (alias: string): Promise<GitHubOrgInfo[]> =>
  call("github_list_orgs", { alias });

export const repairRunnerScope = (runnerId: string): Promise<RunnerProfile> =>
  call("runner_repair_scope", { runnerId });

export const downloadRunner = (
  runnerId: string,
  version?: string
): Promise<RunnerProfile> => call("runner_download", { runnerId, version });

export const configureRunner = (params: {
  runnerId: string;
  scope: RunnerScope;
  name: string;
  labels: string[];
  workDir: string;
}): Promise<RunnerProfile> =>
  call("runner_configure", {
    runnerId: params.runnerId,
    scope: params.scope,
    name: params.name,
    labels: params.labels,
    workDir: params.workDir,
  });

export const startRunner = (runnerId: string): Promise<RuntimeState> =>
  call("runner_start", { runnerId });

export const stopRunner = (runnerId: string): Promise<RuntimeState> =>
  call("runner_stop", { runnerId });

export const fetchRunnerStatus = (runnerId: string): Promise<RuntimeState> =>
  call("runner_status", { runnerId });

export const fetchRunnerStatusAll = (): Promise<Record<string, RuntimeState>> =>
  call("runner_status_all");

export const installService = (runnerId: string): Promise<void> =>
  call("service_install", { runnerId });

export const setRunOnBoot = (runnerId: string, enabled: boolean): Promise<void> =>
  call("service_enable_on_boot", { runnerId, enabled });

export const fetchServiceStatus = (runnerId: string): Promise<ServiceStatus> =>
  call("service_status", { runnerId });

export const fetchServiceStatusAll = (): Promise<Record<string, ServiceStatus>> =>
  call("service_status_all");

export const listLogSources = (runnerId: string): Promise<LogSource[]> =>
  call("logs_list_sources", { runnerId });

export const tailLogs = (
  runnerId: string,
  source: string,
  limit?: number
): Promise<LogLine[]> => call("logs_tail", { runnerId, source, limit });

export const discoverScan = (): Promise<DiscoveryCandidate[]> => call("discover_scan");

export const discoverImport = (
  candidateId: string,
  options: {
    replace_service: boolean;
    move_install: boolean;
    verify_after_move?: boolean;
    delete_original_after_verify?: boolean;
  }
): Promise<string> => call("discover_import", { candidateId, options });

export const discoverMigrateService = (
  runnerId: string,
  strategy: ServiceMigrationStrategy
): Promise<void> => call("discover_migrate_service", { runnerId, strategy });

export const discoverRemoveExternalArtifacts = (runnerId: string): Promise<void> =>
  call("discover_remove_external_artifacts", { runnerId });

export const discoverVerifyRunner = (
  runnerId: string
): Promise<{ ok: boolean; reason?: string | null }> =>
  call("discover_verify_runner", { runnerId });

export const discoverDeleteOriginalInstall = (runnerId: string): Promise<void> =>
  call("discover_delete_original_install", { runnerId });

export const discoverMoveInstall = (
  runnerId: string,
  destination?: string
): Promise<RunnerProfile> => call("discover_move_install", { runnerId, destination });

export const discoverRollbackMove = (runnerId: string): Promise<RunnerProfile> =>
  call("discover_rollback_move", { runnerId });

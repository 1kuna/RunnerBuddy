import { invoke } from "@tauri-apps/api/core";

export type RunnerStatus = "offline" | "idle" | "running";

export type AdoptionDefault = "adopt" | "move_verify_delete";

export type RunnerScope =
  | { type: "repo"; owner: string; repo: string }
  | { type: "org"; org: string }
  | { type: "enterprise"; enterprise: string };

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

export type ServiceMigrationStrategy = "keepexternal" | "replacewithrunnerbuddy";

export async function getState(): Promise<AppSnapshot> {
  return invoke("app_get_state");
}

export async function runnersList(): Promise<AppSnapshot> {
  return invoke("runners_list");
}

export async function getSettings(): Promise<SettingsSnapshot> {
  return invoke("settings_get");
}

export async function updateSettings(
  patch: Partial<SettingsConfig>
): Promise<SettingsSnapshot> {
  return invoke("settings_update", { patch });
}

export async function completeOnboarding(): Promise<SettingsSnapshot> {
  return invoke("onboarding_complete");
}

export async function resetOnboarding(): Promise<SettingsSnapshot> {
  return invoke("onboarding_reset");
}

export async function createRunnerProfile(params: {
  display_name?: string;
  runner_name?: string;
  labels?: string[];
  work_dir?: string;
  scope?: RunnerScope | null;
  pat_alias?: string;
}): Promise<string> {
  return invoke("runners_create_profile", { input: params });
}

export async function updateRunnerProfile(
  runnerId: string,
  patch: {
    display_name?: string;
    runner_name?: string;
    labels?: string[];
    work_dir?: string;
    scope?: RunnerScope | null;
    pat_alias?: string;
  }
): Promise<RunnerProfile> {
  return invoke("runners_update_profile", { runnerId, patch });
}

export async function deleteRunnerProfile(
  runnerId: string,
  mode: RunnerDeleteMode
): Promise<void> {
  return invoke("runners_delete_profile", { runnerId, mode });
}

export async function selectRunner(runnerId: string | null): Promise<void> {
  return invoke("runners_select", { runnerId });
}

export async function savePat(alias: string, pat: string): Promise<void> {
  return invoke("auth_save_pat", { alias, pat });
}

export async function clearPat(alias: string): Promise<void> {
  return invoke("auth_clear_pat", { alias });
}

export async function checkPat(alias: string): Promise<boolean> {
  return invoke("auth_check_pat", { alias });
}

export async function setDefaultPatAlias(alias: string): Promise<void> {
  return invoke("auth_set_default_alias", { alias });
}

export async function downloadRunner(
  runnerId: string,
  version?: string
): Promise<RunnerProfile> {
  return invoke("runner_download", { runnerId, version });
}

export async function configureRunner(params: {
  runnerId: string;
  scope: RunnerScope;
  name: string;
  labels: string[];
  workDir: string;
}): Promise<RunnerProfile> {
  return invoke("runner_configure", {
    runnerId: params.runnerId,
    scope: params.scope,
    name: params.name,
    labels: params.labels,
    workDir: params.workDir,
  });
}

export async function startRunner(runnerId: string): Promise<RuntimeState> {
  return invoke("runner_start", { runnerId });
}

export async function stopRunner(runnerId: string): Promise<RuntimeState> {
  return invoke("runner_stop", { runnerId });
}

export async function fetchRunnerStatus(runnerId: string): Promise<RuntimeState> {
  return invoke("runner_status", { runnerId });
}

export async function fetchRunnerStatusAll(): Promise<Record<string, RuntimeState>> {
  return invoke("runner_status_all");
}

export async function installService(runnerId: string): Promise<void> {
  return invoke("service_install", { runnerId });
}

export async function uninstallService(runnerId: string): Promise<void> {
  return invoke("service_uninstall", { runnerId });
}

export async function setRunOnBoot(
  runnerId: string,
  enabled: boolean
): Promise<void> {
  return invoke("service_enable_on_boot", { runnerId, enabled });
}

export async function fetchServiceStatus(
  runnerId: string
): Promise<ServiceStatus> {
  return invoke("service_status", { runnerId });
}

export async function fetchServiceStatusAll(): Promise<Record<string, ServiceStatus>> {
  return invoke("service_status_all");
}

export async function startService(runnerId: string): Promise<void> {
  return invoke("service_start", { runnerId });
}

export async function stopService(runnerId: string): Promise<void> {
  return invoke("service_stop", { runnerId });
}

export async function listLogSources(runnerId: string): Promise<LogSource[]> {
  return invoke("logs_list_sources", { runnerId });
}

export async function tailLogs(
  runnerId: string,
  source: string,
  limit?: number
): Promise<LogLine[]> {
  return invoke("logs_tail", { runnerId, source, limit });
}

export async function discoverScan(): Promise<DiscoveryCandidate[]> {
  return invoke("discover_scan");
}

export async function discoverImport(
  candidateId: string,
  options: { replace_service: boolean; move_install: boolean; verify_after_move?: boolean; delete_original_after_verify?: boolean }
): Promise<string> {
  return invoke("discover_import", { candidateId, options });
}

export async function discoverAdopt(
  candidateId: string,
  options: { strategy: AdoptionDefault; replace_service: boolean; delete_original_after_verify?: boolean }
): Promise<string> {
  return invoke("discover_adopt", { candidateId, options });
}

export async function discoverMigrateService(
  runnerId: string,
  strategy: ServiceMigrationStrategy
): Promise<void> {
  return invoke("discover_migrate_service", { runnerId, strategy });
}

export async function discoverRemoveExternalArtifacts(runnerId: string): Promise<void> {
  return invoke("discover_remove_external_artifacts", { runnerId });
}

export async function discoverVerifyRunner(
  runnerId: string
): Promise<{ ok: boolean; reason?: string | null }> {
  return invoke("discover_verify_runner", { runnerId });
}

export async function discoverDeleteOriginalInstall(runnerId: string): Promise<void> {
  return invoke("discover_delete_original_install", { runnerId });
}

export async function discoverMoveInstall(
  runnerId: string,
  destination?: string
): Promise<RunnerProfile> {
  return invoke("discover_move_install", { runnerId, destination });
}

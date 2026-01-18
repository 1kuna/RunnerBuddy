import type { RunnerScope } from "./api";

export function scopeLabel(scope?: RunnerScope | null): string {
  if (!scope) return "Scope unknown";
  if (scope.type === "repo") return `${scope.owner}/${scope.repo}`;
  if (scope.type === "org") return scope.org;
  return scope.enterprise;
}

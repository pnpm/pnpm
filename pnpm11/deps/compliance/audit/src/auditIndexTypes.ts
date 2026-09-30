import type { DepTypes } from '@pnpm/lockfile.detect-dep-types'
import type { EnvLockfile } from '@pnpm/lockfile.types'
import type { DependenciesField, DepPath } from '@pnpm/types'

export interface PathInfo {
  paths: string[]
  dev: boolean
  optional: boolean
}

// Versions installed per package name, keyed by version.
export type AuditPathIndex = Record<string, Map<string, PathInfo>>

export interface AuditIndexRequest {
  // Flat map suitable as the POST body for `/advisories/bulk`.
  request: Record<string, string[]>
  totalDependencies: number
  // Production dependencies: neither dev-only nor optional-only. Kept as a
  // distinct counter because devOnly and optionalOnly aren't mutually
  // exclusive — a (name, version) can be both — so `total - dev - optional`
  // would double-subtract those entries.
  dependencies: number
  devDependencies: number
  optionalDependencies: number
}

export interface AuditIndexOptions {
  envLockfile?: EnvLockfile | null
  include?: { [dependenciesField in DependenciesField]: boolean }
  resolvePeersFromWorkspaceRoot?: boolean
  // Pre-computed dep types. Callers that also call buildAuditPathIndex on the
  // same lockfile can share this to avoid walking the lockfile twice.
  depTypes?: DepTypes
  // Pre-computed optional-only depPaths for the main lockfile. Shared between
  // lockfileToAuditRequest and buildAuditPathIndex when both are called.
  optionalOnly?: Set<DepPath>
}

export interface DependencyEdge {
  name: string
  depPath: DepPath
}

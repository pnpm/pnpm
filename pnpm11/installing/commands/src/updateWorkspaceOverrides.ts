import { readWantedLockfile, writeWantedLockfile } from '@pnpm/lockfile.fs'
import type { ProjectManifest } from '@pnpm/types'

export type WorkspaceOverrideUpdateCandidates = Map<string, Set<string>>

export interface ProjectManifestChange {
  before: ProjectManifest
  after: ProjectManifest
}

export interface WorkspaceOverrideUpdateConflict {
  alias: string
  specifiers: string[]
}

interface WorkspaceOverrideUpdateOptions {
  onConflict?: (conflict: WorkspaceOverrideUpdateConflict) => void
}

export interface PickUpdatedLockfileWorkspaceOverridesOptions extends WorkspaceOverrideUpdateOptions {
  lockfileDir: string
  overrides: Record<string, string> | undefined
  projects: ProjectManifestChange[]
  useGitBranchLockfile?: boolean
  mergeGitBranchLockfiles?: boolean
}

interface LockfileWriteOptions extends WorkspaceOverrideUpdateOptions {
  useGitBranchLockfile?: boolean
  mergeGitBranchLockfiles?: boolean
}

export function addUpdatedWorkspaceOverrideCandidates (
  candidates: WorkspaceOverrideUpdateCandidates,
  updatedOverrides: Record<string, string> | undefined
): void {
  if (updatedOverrides == null) return

  for (const [alias, nextSpecifier] of Object.entries(updatedOverrides)) {
    if (typeof nextSpecifier !== 'string') continue
    addUpdatedWorkspaceOverrideCandidate(candidates, alias, nextSpecifier)
  }
}

export function pickUniqueUpdatedWorkspaceOverrides (
  candidates: WorkspaceOverrideUpdateCandidates,
  opts?: WorkspaceOverrideUpdateOptions
): Record<string, string> | undefined {
  const updatedOverrides = createSafeStringRecord()
  for (const [alias, values] of candidates) {
    if (values.size !== 1) {
      opts?.onConflict?.({
        alias,
        specifiers: Array.from(values).sort(),
      })
      continue
    }
    setOwnString(updatedOverrides, alias, Array.from(values)[0]!)
  }

  return Object.keys(updatedOverrides).length > 0 ? updatedOverrides : undefined
}

export function pickUpdatedWorkspaceOverrides (
  overrides: Record<string, string> | undefined,
  projects: ProjectManifestChange[],
  opts?: WorkspaceOverrideUpdateOptions
): Record<string, string> | undefined {
  if (overrides == null || Object.keys(overrides).length === 0) return undefined

  const candidates: WorkspaceOverrideUpdateCandidates = new Map()
  for (const { before, after } of projects) {
    const previousDependencies = getDirectDependenciesForOverrides(before)
    const nextDependencies = getDirectDependenciesForOverrides(after)
    for (const [alias, nextSpecifier] of Object.entries(nextDependencies)) {
      const previousSpecifier = getOwnString(previousDependencies, alias)
      if (previousSpecifier == null || previousSpecifier === nextSpecifier) continue
      if (getOwnString(overrides, alias) !== previousSpecifier) continue

      addUpdatedWorkspaceOverrideCandidate(candidates, alias, nextSpecifier)
    }
  }

  return pickUniqueUpdatedWorkspaceOverrides(candidates, opts)
}

export async function pickUpdatedLockfileWorkspaceOverrides ({
  lockfileDir,
  overrides,
  projects,
  ...opts
}: PickUpdatedLockfileWorkspaceOverridesOptions): Promise<Record<string, string> | undefined> {
  if (overrides == null || Object.keys(overrides).length === 0) return undefined

  const wantedLockfile = await readWantedLockfile(lockfileDir, {
    ignoreIncompatible: false,
    ...opts,
  })
  if (wantedLockfile?.overrides == null) return undefined

  const candidates: WorkspaceOverrideUpdateCandidates = new Map()
  for (const { before, after } of projects) {
    const previousDependencies = getDirectDependenciesForOverrides(before)
    const nextDependencies = getDirectDependenciesForOverrides(after)
    for (const alias of Object.keys(nextDependencies)) {
      const previousSpecifier = getOwnString(previousDependencies, alias)
      const nextDependencySpecifier = getOwnString(nextDependencies, alias)
      const nextSpecifier = getOwnString(wantedLockfile.overrides, alias)
      if (previousSpecifier == null || nextDependencySpecifier == null || previousSpecifier === nextDependencySpecifier) continue
      if (nextSpecifier == null || nextSpecifier !== nextDependencySpecifier) continue
      if (getOwnString(overrides, alias) !== previousSpecifier) continue

      addUpdatedWorkspaceOverrideCandidate(candidates, alias, nextSpecifier)
    }
  }

  return pickUniqueUpdatedWorkspaceOverrides(candidates, opts)
}

export async function writeUpdatedLockfileOverrides (
  lockfileDir: string,
  updatedOverrides: Record<string, string> | undefined,
  opts?: LockfileWriteOptions
): Promise<void> {
  if (updatedOverrides == null || Object.keys(updatedOverrides).length === 0) return

  const wantedLockfile = await readWantedLockfile(lockfileDir, {
    ignoreIncompatible: false,
    ...opts,
  })
  if (wantedLockfile == null) return

  wantedLockfile.overrides = mergeStringRecords(wantedLockfile.overrides, updatedOverrides)
  await writeWantedLockfile(lockfileDir, wantedLockfile, opts)
}

export function shouldWriteUpdatedLockfileOverrides (
  installerUpdatedOverrides: Record<string, string> | undefined,
  workspaceOverrides: Record<string, string> | undefined
): boolean {
  return installerUpdatedOverrides == null &&
    workspaceOverrides != null &&
    Object.keys(workspaceOverrides).length > 0
}

function getDirectDependenciesForOverrides (manifest: ProjectManifest): Record<string, string> {
  return mergeStringRecords(
    manifest.devDependencies,
    manifest.dependencies,
    manifest.optionalDependencies
  )
}

function addUpdatedWorkspaceOverrideCandidate (
  candidates: WorkspaceOverrideUpdateCandidates,
  alias: string,
  nextSpecifier: string
): void {
  if (typeof alias !== 'string' || typeof nextSpecifier !== 'string') return

  let values = candidates.get(alias)
  if (values == null) {
    values = new Set()
    candidates.set(alias, values)
  }
  values.add(nextSpecifier)
}

function mergeStringRecords (...records: Array<Record<string, string> | undefined>): Record<string, string> {
  const result = createSafeStringRecord()
  for (const record of records) {
    if (record == null) continue
    for (const key of Object.keys(record)) {
      const value = getOwnString(record, key)
      if (value == null) continue
      setOwnString(result, key, value)
    }
  }
  return result
}

function createSafeStringRecord (): Record<string, string> {
  return Object.create(null) as Record<string, string>
}

function getOwnString (record: Record<string, string>, key: string): string | undefined {
  const value = Object.prototype.propertyIsEnumerable.call(record, key) ? record[key] : undefined
  return typeof value === 'string' ? value : undefined
}

function setOwnString (record: Record<string, string>, key: string, value: string): void {
  Object.defineProperty(record, key, {
    value,
    writable: true,
    enumerable: true,
    configurable: true,
  })
}

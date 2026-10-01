import type { PnpmContext } from '@pnpm/installing.context'
import type { UpdateMatchingFunction } from '@pnpm/installing.deps-resolver'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { ChangedField, ChangedSettingsField } from '@pnpm/lockfile.settings-checker'
import type { ResolutionPolicyViolation } from '@pnpm/resolving.resolver-base'
import {
  DEPENDENCIES_FIELDS,
  type DependenciesField,
  type IncludedDependencies,
  type ProjectManifest,
  type ProjectRootDir,
  type ReadPackageHook,
} from '@pnpm/types'
import { isEmpty } from 'ramda'

import type { ProcessedInstallOptions as StrictInstallOptions } from './extendInstallOptions.js'
import type { ImporterToUpdate, MutatedProject, UninstallSomeDepsMutation } from './mutationTypes.js'

export const DEV_PREINSTALL = 'pnpm:devPreinstall'

/**
 * A "check-only" install resolves fully but writes nothing: `dryRun`
 * (`pnpm install --dry-run`) and `lockfileCheck` (`pnpm dedupe --check`)
 * both take this path. The shared flag suppresses every write and forces a
 * full resolution (the frozen/headless fast paths are skipped) so the wanted
 * lockfile can always be compared.
 */
export function isCheckOnlyInstall (opts: { lockfileCheck?: unknown, dryRun?: boolean }): boolean {
  return opts.lockfileCheck != null || opts.dryRun === true
}

/**
 * `pnpm:devPreinstall` prepares a development checkout, so an install that
 * leaves out `devDependencies`, such as `pnpm install --prod`, skips it.
 */
export function installRunsDevPreinstall (
  opts: { ignoreScripts?: boolean, ignorePackageManifest?: boolean, include?: IncludedDependencies }
): boolean {
  return !opts.ignoreScripts && !opts.ignorePackageManifest && opts.include?.devDependencies !== false
}

/**
 * Whether the root project's `preinstall` runs ahead of resolution: only when
 * the root would run its own lifecycle scripts after linking, so `pnpm add`
 * and `pnpm remove` keep their behavior. A check-only or lockfile-only
 * install materializes nothing, so it runs no project script.
 */
export function rootProjectRunsPreinstallEarly (
  projects: Array<{ rootDir: ProjectRootDir, mutation: MutatedProject['mutation'] }>,
  opts: {
    ignoreScripts?: boolean
    ignorePackageManifest?: boolean
    lockfileCheck?: unknown
    lockfileDir: string
    lockfileOnly?: boolean
    dryRun?: boolean
    virtualStoreOnly?: boolean
  }
): boolean {
  return !opts.ignoreScripts && !opts.ignorePackageManifest && !opts.virtualStoreOnly &&
    !opts.lockfileOnly && !isCheckOnlyInstall(opts) &&
    projects.some((project) => project.rootDir === opts.lockfileDir && project.mutation === 'install')
}

/**
 * Whether the install materializes fewer dependency groups than it resolves.
 * Resolution walks every group so the lockfile keeps describing the manifest
 * rather than the `--prod` / `--dev` filter the run was invoked with, which
 * leaves materialization as the only stage the filter can reach. Until it
 * does, the resolver fetches the tarballs of the groups the install drops
 * (pnpm/pnpm#881).
 *
 * A filter no importer has anything to drop to drops nothing, and such an
 * install keeps its single pass: `pnpm add -g` excludes `devDependencies`
 * from a manifest it writes `dependencies` into and nothing else.
 * `optionalDependencies` are the exception, because every package in the
 * graph can declare one and dropping the group drops those too, which no
 * importer's manifest shows. An importer's own `optionalDependencies` drop
 * with its `dependencies`.
 */
export function materializesGroupSubset (include: IncludedDependencies, projects: ImporterToUpdate[]): boolean {
  if (!include.optionalDependencies) return true
  return projects.some(({ manifest }) =>
    (!include.dependencies && (!isEmpty(manifest.dependencies ?? {}) || !isEmpty(manifest.optionalDependencies ?? {}))) ||
    (!include.devDependencies && !isEmpty(manifest.devDependencies ?? {}))
  )
}

export function policyViolationKey ({ code, name, version }: ResolutionPolicyViolation): string {
  return `${code}:${name}@${version}`
}

export function allMutationsAreInstalls (projects: MutatedProject[]): boolean {
  return projects.every((project) => project.mutation === 'install' && !project.update && !project.updateMatching)
}

export function hasUninstallMutations (projects: MutatedProject[]): boolean {
  return projects.some((project) => project.mutation === 'uninstallSome')
}

/**
 * Whether the install reads a lockfile and that lockfile is a single file,
 * not one per git branch. An unset setting counts as its default.
 */
export function usesSingleLockfile (opts: { mergeGitBranchLockfiles?: boolean, useGitBranchLockfile?: boolean, useLockfile?: boolean }): boolean {
  return opts.useLockfile !== false &&
    opts.useGitBranchLockfile !== true &&
    opts.mergeGitBranchLockfiles !== true
}

/**
 * Whether pacquet resolves this install itself instead of materializing a
 * lockfile pnpm resolved. The caller adds the guards every delegation shares
 * (`enableModulesDir`, not `lockfileOnly`, not a check-only run).
 *
 * `mutateModules` waives this install's lockfile verification on the strength
 * of it, because pacquet applies the resolver policy as it resolves. A branch
 * that resolves in pnpm has to keep out of the way of this one, or the install
 * both loses pacquet's resolution and is never verified.
 */
export function pacquetResolvesInstall (
  projects: MutatedProject[],
  opts: Pick<StrictInstallOptions, 'frozenLockfile' | 'handleResolutionPolicyViolations' | 'mergeGitBranchLockfiles' | 'runPacquet' | 'saveLockfile' | 'useGitBranchLockfile' | 'useLockfile'>
): boolean {
  return opts.runPacquet?.supportsResolution === true &&
    opts.useLockfile &&
    opts.saveLockfile &&
    !opts.useGitBranchLockfile &&
    !opts.mergeGitBranchLockfiles &&
    !opts.frozenLockfile &&
    opts.handleResolutionPolicyViolations == null &&
    allMutationsAreInstalls(projects)
}

/**
 * Matches the lockfile entries this update re-resolves at every place they
 * occur, so none of their locked versions can be reused. Returns
 * `undefined` when a locked version of a matched package may survive: a
 * `--depth` limit, a lockfile importer the update does not cover, or a
 * project that is not updating by package name.
 */
export function matchUpdateTargetsReplacedEverywhere (
  projects: MutatedProject[],
  ctx: Pick<PnpmContext, 'projects' | 'wantedLockfile'>,
  depth: number
): ((name: string, version: string) => boolean) | undefined {
  if (depth !== Infinity || projects.length === 0) return undefined
  const updateMatchings = new Set<UpdateMatchingFunction>()
  for (const project of projects) {
    if (project.mutation === 'uninstallSome' || project.updateMatching == null) return undefined
    updateMatchings.add(project.updateMatching)
  }
  const updatedImporterIds = new Set<string | undefined>(projects.map(({ rootDir }) => ctx.projects[rootDir]?.id))
  if (Object.keys(ctx.wantedLockfile.importers ?? {}).some((importerId) => !updatedImporterIds.has(importerId))) {
    return undefined
  }
  const distinctUpdateMatchings = [...updateMatchings]
  return (name, version) => distinctUpdateMatchings.every((updateMatching) => updateMatching(name, version))
}

export function removesAnyDependency (project: UninstallSomeDepsMutation, manifest: ProjectManifest | undefined): boolean {
  if (manifest == null) return false
  const fields: Array<DependenciesField | 'peerDependencies'> = project.targetDependenciesField != null
    ? [project.targetDependenciesField, 'peerDependencies']
    : [...DEPENDENCIES_FIELDS, 'peerDependencies']
  return project.dependencyNames.some((name) => fields.some((field) => manifest[field]?.[name] != null))
}

export function isSettingsField (changedField: ChangedField): changedField is ChangedSettingsField {
  return changedField.startsWith('settings.')
}

export function cacheExpired (prunedAt: string, maxAgeInMinutes: number): boolean {
  return ((Date.now() - new Date(prunedAt).valueOf()) / (1000 * 60)) > maxAgeInMinutes
}

export function pkgHasDependencies (manifest: ProjectManifest): boolean {
  return Boolean(
    (Object.keys(manifest.dependencies ?? {}).length > 0) ||
    Object.keys(manifest.devDependencies ?? {}).length ||
    Object.keys(manifest.optionalDependencies ?? {}).length
  )
}

export function getUntrackedPnpmfileReadPackageHook (
  hooks: StrictInstallOptions['hooks']
): boolean | undefined {
  if (hooks.untrackedPnpmfileReadPackageHook != null) {
    return hooks.untrackedPnpmfileReadPackageHook
  }
  const readPackage = hooks.readPackage as ReadPackageHook[] | ReadPackageHook | undefined
  const hasReadPackage = Array.isArray(readPackage) ? readPackage.length > 0 : readPackage != null
  return hooks.calculatePnpmfileChecksum == null && hasReadPackage ? true : undefined
}

export function setUntrackedPnpmfileReadPackageHook (
  lockfile: LockfileObject,
  value: boolean | undefined
): void {
  if (value == null) {
    delete lockfile.untrackedPnpmfileReadPackageHook
  } else {
    lockfile.untrackedPnpmfileReadPackageHook = value
  }
}

/**
 * Whether the pnpmfile's `readPackage` hook, as recorded in the lockfile, may
 * differ from the one this run applies: the pnpmfile checksum cannot vouch
 * for it, or the lockfile recorded a different answer.
 */
export function readPackageHookMayHaveChanged (
  wantedLockfile: LockfileObject,
  untrackedPnpmfileReadPackageHook: boolean | undefined
): boolean {
  return untrackedPnpmfileReadPackageHook === true ||
    wantedLockfile.untrackedPnpmfileReadPackageHook !== untrackedPnpmfileReadPackageHook
}

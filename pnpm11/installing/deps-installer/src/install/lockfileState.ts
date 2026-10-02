import { hashObjectNullableWithPrefix } from '@pnpm/crypto.object-hasher'
import type { PnpmContext } from '@pnpm/installing.context'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import {
  calcPatchHashes,
  type ChangedField,
  createOverridesMapFromParsed,
  getOutdatedLockfileSettings,
  resolvePatchedDependencies,
} from '@pnpm/lockfile.settings-checker'
import type { allProjectsAreUpToDate } from '@pnpm/lockfile.verification'
import { groupPatchedDependenciesWithPaths, type PatchGroupRecord } from '@pnpm/patching.config'
import type { ProjectRootDir } from '@pnpm/types'

import type { ProcessedInstallOptions as StrictInstallOptions } from './extendInstallOptions.js'
import { LockfileConfigMismatchError } from './frozenInstallErrors.js'
import { getUntrackedPnpmfileReadPackageHook, readPackageHookMayHaveChanged } from './installPredicates.js'
import type { MutationRun } from './mutationTypes.js'
import { type AddedManifests, tryAddLockedVersions } from './tryAddLockedVersions.js'

export interface InstallChecksums {
  frozenLockfile: boolean
  packageExtensionsChecksum: string | undefined
  patchedDependencies: Record<string, string> | undefined
  patchGroups: PatchGroupRecord | undefined
  pnpmfileChecksum: string | undefined
  pnpmfileChecksumIgnored: boolean | undefined
  untrackedPnpmfileReadPackageHook: boolean | undefined
  untrackedReadPackageHookMayHaveChanged: boolean
}

export async function readInstallChecksums ({ ctx, opts }: Pick<MutationRun, 'ctx' | 'opts'>): Promise<InstallChecksums> {
  const packageExtensionsChecksum = hashObjectNullableWithPrefix(opts.packageExtensions)
  const pnpmfileChecksum = await opts.hooks.calculatePnpmfileChecksum?.()
  // `ignorePnpmfile` skips the pnpmfile for this run only, so the checksum
  // the lockfile records is not compared against its absence, and a
  // lockfile that is otherwise up to date installs as it is. A run that
  // changes the lockfile resolves without the snapshots the pnpmfile shaped
  // and records no checksum (https://github.com/pnpm/pnpm/issues/10944).
  const pnpmfileChecksumIgnored = opts.ignorePnpmfile && pnpmfileChecksum == null &&
    ctx.wantedLockfile.pnpmfileChecksum != null
  const untrackedPnpmfileReadPackageHook = getUntrackedPnpmfileReadPackageHook(opts.hooks)
  const untrackedReadPackageHookMayHaveChanged = readPackageHookMayHaveChanged(ctx.wantedLockfile, untrackedPnpmfileReadPackageHook)
  const resolvedPatchedDeps = resolvePatchedDependencies(opts.patchedDependencies, opts.lockfileDir)
  const patchedDependencies = opts.ignorePackageManifest
    ? ctx.wantedLockfile.patchedDependencies
    : (resolvedPatchedDeps ? await calcPatchHashes(resolvedPatchedDeps) : {})
  const patchGroups = groupPatchedDependenciesWithPaths(patchedDependencies, resolvedPatchedDeps)
  const frozenLockfile = opts.frozenLockfile ||
    opts.frozenLockfileIfExists && ctx.existsNonEmptyWantedLockfile
  return {
    frozenLockfile,
    packageExtensionsChecksum,
    patchedDependencies,
    patchGroups,
    pnpmfileChecksum,
    pnpmfileChecksumIgnored,
    untrackedPnpmfileReadPackageHook,
    untrackedReadPackageHookMayHaveChanged,
  }
}

export interface StagedAdditions {
  addedManifests: Map<ProjectRootDir, AddedManifests> | null
  /**
   * Whether every `installSome` mutation's manifest edit has been applied
   * to the context. Without it the up-to-date check would not see the
   * requested dependencies, and a frozen-like install would leave them
   * uninstalled.
   */
  addedManifestsAreCommitted: boolean
  resolutionPicksLowest: boolean
}

export function stageAddedManifests ({ ctx, opts, projects }: Pick<MutationRun, 'ctx' | 'opts' | 'projects'>): StagedAdditions {
  // `time-based` and `lowest-direct` resolve a direct dependency to its
  // lowest satisfying version, so the lockfile alone does not say which end
  // of a range a rewrite should reuse.
  const resolutionPicksLowest = opts.resolutionMode !== 'highest'
  // `installSome` cannot edit the manifests in place the way `uninstallSome`
  // does: the resolution path reads them to tell a new dependency from a
  // re-added one, and reads them again to pick the range style it saves. So
  // the additions the lockfile can answer on its own are staged on copies
  // here, handed to the fast update and its freshness gates below, and
  // committed to the context only once that rewrite succeeds.
  const installSomeProjects = projects.filter((project) => project.mutation === 'installSome')
  const addedManifests = installSomeProjects.length === 0
    ? new Map<ProjectRootDir, AddedManifests>()
    : tryAddLockedVersions(ctx.wantedLockfile, {
      added: installSomeProjects.map((project) => ({
        ...project,
        manifest: ctx.projects[project.rootDir].manifest,
        originalManifest: ctx.projects[project.rootDir].originalManifest,
      })),
      autoInstallPeers: opts.autoInstallPeers,
      catalogs: opts.catalogs,
      catalogMode: opts.catalogMode,
      ignoreCurrentSpecifiers: opts.ignoreCurrentSpecifiers,
      parsedOverrides: opts.parsedOverrides,
      resolutionPicksLowest,
      saveCatalogName: opts.saveCatalogName,
      workspacePackages: ctx.workspacePackages,
    })
  return {
    addedManifests,
    addedManifestsAreCommitted: installSomeProjects.length === 0,
    resolutionPicksLowest,
  }
}

export interface LockfileSettingsState {
  changedLockfileSettings: ChangedField[]
  overridesMap: Record<string, string>
  wantedLockfileSettings: WantedLockfileSettings
}

type WantedLockfileSettings = NonNullable<LockfileObject['settings']>

export function detectOutdatedLockfileSettings (
  { ctx, opts }: Pick<MutationRun, 'ctx' | 'opts'>,
  checksums: InstallChecksums
): LockfileSettingsState {
  const overridesMap = createOverridesMapFromParsed(opts.parsedOverrides)
  const wantedLockfileSettings: WantedLockfileSettings = {
    autoInstallPeers: opts.autoInstallPeers,
    dedupePeers: opts.dedupePeers || undefined,
    excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
    peersSuffixMaxLength: opts.peersSuffixMaxLength,
    injectWorkspacePackages: opts.injectWorkspacePackages,
  }
  const lockfileSettings = {
    ...wantedLockfileSettings,
    catalogs: opts.catalogs,
    ignoredOptionalDependencies: opts.ignoredOptionalDependencies?.sort(),
    packageExtensionsChecksum: checksums.packageExtensionsChecksum,
    patchedDependencies: checksums.patchedDependencies,
    pnpmfileChecksum: checksums.pnpmfileChecksum,
  }
  let changedLockfileSettings: ChangedField[] = []
  if (!opts.ignorePackageManifest) {
    changedLockfileSettings = getOutdatedLockfileSettings(ctx.wantedLockfile, {
      ...lockfileSettings,
      ignorePnpmfileChecksum: checksums.pnpmfileChecksumIgnored,
      overrides: overridesMap,
    })
    if (checksums.frozenLockfile && changedLockfileSettings.length > 0) {
      throw new LockfileConfigMismatchError(changedLockfileSettings[0])
    }
  }
  return { changedLockfileSettings, overridesMap, wantedLockfileSettings }
}

/**
 * The options `allProjectsAreUpToDate` compares the projects of this install
 * against `wantedLockfile` with.
 */
export function upToDateCheckOptions (
  opts: StrictInstallOptions,
  { ctx, wantedLockfile }: { ctx: Pick<PnpmContext, 'workspacePackages'>, wantedLockfile: LockfileObject }
): Parameters<typeof allProjectsAreUpToDate>[1] {
  return {
    catalogs: opts.catalogs,
    autoInstallPeers: opts.autoInstallPeers,
    excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
    ignoredOptionalDependencies: opts.ignoredOptionalDependencies,
    linkWorkspacePackages: opts.linkWorkspacePackagesDepth >= 0,
    wantedLockfile,
    workspacePackages: ctx.workspacePackages,
    lockfileDir: opts.lockfileDir,
    workspaceDir: opts.workspaceDir,
  }
}

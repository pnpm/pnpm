import path from 'node:path'

import { installabilityUnderForce } from '@pnpm/config.package-is-installable'
import { MANIFEST_BASE_NAMES, WANTED_LOCKFILE } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import type { PnpmContext } from '@pnpm/installing.context'
import { filterLockfileByImportersAndEngine } from '@pnpm/lockfile.filtering'
import type { LockfileObject, ProjectSnapshot } from '@pnpm/lockfile.fs'
import {
  catalogResolutionsAreUpToDate,
  checkLinkedPackagesAreUpToDate,
  findPackageTarballIntegrityMismatch,
  getWorkspacePackagesByDirectory,
  satisfiesPackageManifest,
  unresolvedOptionalDependencies,
} from '@pnpm/lockfile.verification'
import type { ProjectId, ProjectManifest, ProjectRootDir } from '@pnpm/types'
import { TarballIntegrityError } from '@pnpm/worker'
import pLimit from 'p-limit'
import { pathExists } from 'path-exists'

import { getCurrentEngine } from './currentEngine.js'
import type { ProcessedInstallOptions as StrictInstallOptions } from './extendInstallOptions.js'
import { pkgHasDependencies } from './installPredicates.js'
import type { MutationRun } from './mutationTypes.js'

const FROZEN_LOCKFILE_HINT = 'Note that in CI environments this setting is true by default. If you still need to run install in such cases, use "pnpm install --no-frozen-lockfile"'

export interface SkippedOptionalDependencies {
  prefix: string
  skipped: Record<string, string>
}

interface ProjectCheck {
  satisfies: boolean
  detailedReason?: string
  rootDir: ProjectRootDir
  skipped?: Record<string, string>
}

type CheckLinkedPackages = (opts: { dir: string, manifest: ProjectManifest, snapshot: ProjectSnapshot }) => ReturnType<typeof checkLinkedPackagesAreUpToDate>

/**
 * Checks that the lockfile describes every project's manifest, and returns
 * the optional dependencies the install that wrote the lockfile could not
 * resolve.
 */
export async function checkProjectsAgainstLockfile (run: MutationRun, frozenLockfile: boolean): Promise<SkippedOptionalDependencies[]> {
  const { ctx, opts } = run
  // `--frozen-lockfile` (the CI default) means "fail if pnpm-lock.yaml is
  // out of sync." Treat its absence as a sync failure even when the
  // synthesized snapshot from node_modules/.pnpm/lock.yaml would satisfy
  // the manifest — the developer needs to commit the regenerated file.
  if (frozenLockfile && !ctx.existsWantedLockfile &&
    Object.values(ctx.projects).some((project) => pkgHasDependencies(project.manifest))) {
    throw createNoLockfileError()
  }
  if (frozenLockfile && opts.pruneLockfileImporters) {
    await throwOnImporterWithoutProjectManifest(ctx, opts)
  }
  const projectChecks = await checkEachProjectAgainstLockfile(run, frozenLockfile)

  const skippedOptionalDependencies: SkippedOptionalDependencies[] = []
  for (const { satisfies, detailedReason, rootDir, skipped } of projectChecks) {
    if (skipped) {
      skippedOptionalDependencies.push({
        prefix: rootDir,
        skipped,
      })
    }
    if (!satisfies) {
      throw createOutdatedProjectError(ctx, { detailedReason, lockfileDir: opts.lockfileDir, rootDir })
    }
  }
  return skippedOptionalDependencies
}

function createNoLockfileError (): PnpmError {
  return new PnpmError('NO_LOCKFILE',
    `Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is absent`, {
      hint: FROZEN_LOCKFILE_HINT,
    })
}

function createOutdatedProjectError (
  ctx: PnpmContext,
  { detailedReason, lockfileDir, rootDir }: { detailedReason: string | undefined, lockfileDir: string, rootDir: ProjectRootDir }
): PnpmError {
  if (!ctx.existsWantedLockfile) {
    return createNoLockfileError()
  }

  return new PnpmError('OUTDATED_LOCKFILE',
    `Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is not up to date with ` +
    path.join('<ROOT>', path.relative(lockfileDir, path.join(rootDir, 'package.json'))), {
      hint: `${FROZEN_LOCKFILE_HINT}

  Failure reason:
  ${detailedReason ?? ''}`,
    })
}

async function throwOnImporterWithoutProjectManifest (ctx: PnpmContext, opts: StrictInstallOptions): Promise<void> {
  const removedImporterId = await findImporterWithoutProjectManifest(ctx.wantedLockfile, {
    lockfileDir: opts.lockfileDir,
    projectIds: Object.values(ctx.projects).map(({ id }) => id),
  })
  if (removedImporterId == null) return
  throw new PnpmError('OUTDATED_LOCKFILE',
    `Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is not up to date with ` +
    path.join('<ROOT>', removedImporterId, 'package.json'), {
      hint: `${FROZEN_LOCKFILE_HINT}

  Failure reason:
  The lockfile records importers["${removedImporterId}"], but that project's directory has no package.json`,
    })
}

/**
 * A project whose directory exists without a manifest cannot be installed, so
 * the lockfile no longer describes the workspace. A frozen install skips a
 * project removed from the workspace patterns, which keeps its manifest, and
 * one whose whole directory is absent, as in a Docker build context that
 * excludes it.
 */
async function findImporterWithoutProjectManifest (
  lockfile: LockfileObject,
  opts: { lockfileDir: string, projectIds: string[] }
): Promise<string | undefined> {
  const projectIds = new Set(opts.projectIds)
  for (const importerId of Object.keys(lockfile.importers)) {
    if (projectIds.has(importerId)) continue
    // eslint-disable-next-line no-await-in-loop -- the search stops at the first importer without a manifest
    if (await isDirectoryWithoutManifest(path.join(opts.lockfileDir, importerId))) return importerId
  }
  return undefined
}

async function isDirectoryWithoutManifest (projectDir: string): Promise<boolean> {
  if (!await pathExists(projectDir)) return false
  const manifestExists = await Promise.all(MANIFEST_BASE_NAMES.map(async (basename) => pathExists(path.join(projectDir, basename))))
  return !manifestExists.some(Boolean)
}

async function checkEachProjectAgainstLockfile ({ ctx, opts }: MutationRun, frozenLockfile: boolean): Promise<ProjectCheck[]> {
  const manifestsByDir = ctx.workspacePackages ? getWorkspacePackagesByDirectory(ctx.workspacePackages) : {}
  const checkLinkedPackages: CheckLinkedPackages = checkLinkedPackagesAreUpToDate.bind(null, {
    linkWorkspacePackages: opts.linkWorkspacePackagesDepth >= 0,
    manifestsByDir,
    workspacePackages: ctx.workspacePackages,
    lockfilePackages: ctx.wantedLockfile.packages,
    lockfileDir: opts.lockfileDir,
    workspaceDir: opts.workspaceDir,
    injectWorkspacePackages: opts.injectWorkspacePackages ?? ctx.wantedLockfile.settings?.injectWorkspacePackages,
    skipLocalDirectoryDependencies: true,
  })
  const checkManifest = satisfiesPackageManifest.bind(null, {
    autoInstallPeers: opts.autoInstallPeers,
    excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
    ignoredOptionalDependencies: opts.ignoredOptionalDependencies,
    allowUnresolvedOptionalDependencies: frozenLockfile,
  })
  return Promise.all(Object.values(ctx.projects).map(async ({ id, manifest, rootDir }) => {
    const importer = ctx.wantedLockfile.importers[id]
    const { satisfies, detailedReason } = checkManifest(importer, manifest)
    const skipped = satisfies && frozenLockfile && importer != null
      ? findUnresolvedOptionalDependencies(opts, { importer, manifest })
      : undefined
    if (!satisfies || importer == null) return { satisfies, detailedReason, rootDir, skipped }
    return {
      ...await checkImporterResolutions(ctx, { checkLinkedPackages, detailedReason, importer, manifest, rootDir }),
      rootDir,
      skipped,
    }
  }))
}

function findUnresolvedOptionalDependencies (
  opts: StrictInstallOptions,
  { importer, manifest }: { importer: ProjectSnapshot, manifest: ProjectManifest }
): Record<string, string> | undefined {
  const unresolved = unresolvedOptionalDependencies({
    excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
    ignoredOptionalDependencies: opts.ignoredOptionalDependencies,
  }, importer, manifest)
  return Object.keys(unresolved).length > 0 ? unresolved : undefined
}

async function checkImporterResolutions (
  ctx: PnpmContext,
  { checkLinkedPackages, detailedReason, importer, manifest, rootDir }: {
    checkLinkedPackages: CheckLinkedPackages
    detailedReason: string | undefined
    importer: ProjectSnapshot
    manifest: ProjectManifest
    rootDir: ProjectRootDir
  }
): Promise<{ satisfies: boolean, detailedReason?: string }> {
  if (!catalogResolutionsAreUpToDate(importer, ctx.wantedLockfile.catalogs)) {
    return { satisfies: false, detailedReason: 'Catalog resolutions are not up to date' }
  }
  const linkedResult = await checkLinkedPackages({ dir: rootDir, manifest, snapshot: importer })
  if (!linkedResult.upToDate) {
    return { satisfies: false, detailedReason: linkedResult.detailedReason }
  }
  return { satisfies: true, detailedReason }
}

export async function verifyLockedTarballIntegrity ({ ctx, opts, projects }: MutationRun): Promise<void> {
  const fileIntegrityCache = new Map<string, Promise<string>>()
  const importerIds = opts.ignorePackageManifest === true
    ? Object.keys(ctx.wantedLockfile.importers) as ProjectId[]
    : projects.map(({ rootDir }) => ctx.projects[rootDir].id)
  const skipped = new Set<string>()
  const { lockfile } = filterLockfileByImportersAndEngine(ctx.wantedLockfile, importerIds, {
    include: opts.include,
    currentEngine: getCurrentEngine(opts),
    ...installabilityUnderForce(opts),
    failOnMissingDependencies: false,
    lockfileDir: opts.lockfileDir,
    resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
    skipped,
    skipRuntimes: opts.skipRuntimes,
    supportedArchitectures: opts.supportedArchitectures,
  })
  const limitVerification = pLimit(16)
  await Promise.all(Object.entries(lockfile.packages ?? {}).map(([depPath, snapshot]) => limitVerification(async () => {
    if (skipped.has(depPath)) return
    await throwOnTarballIntegrityMismatch({ fileIntegrityCache, lockfileDir: opts.lockfileDir }, { depPath, snapshot })
  })))
}

async function throwOnTarballIntegrityMismatch (
  opts: Parameters<typeof findPackageTarballIntegrityMismatch>[0],
  { depPath, snapshot }: { depPath: string, snapshot: Parameters<typeof findPackageTarballIntegrityMismatch>[1] }
): Promise<void> {
  const mismatch = await findPackageTarballIntegrityMismatch(opts, snapshot, depPath)
  if (mismatch == null) return
  const algorithm = mismatch.expected.includes('-') ? mismatch.expected.split('-', 1)[0] : 'sha512'
  throw new TarballIntegrityError({
    algorithm,
    expected: mismatch.expected,
    found: mismatch.found,
    sri: mismatch.expected,
    url: mismatch.path,
  })
}

import { parseOverrides, type VersionOverride } from '@pnpm/config.parse-overrides'
import { createProjectConfigRecord } from '@pnpm/config.reader'
import { hashObjectNullableWithPrefix } from '@pnpm/crypto.object-hasher'
import { PnpmError } from '@pnpm/error'
import { createReadPackageHook } from '@pnpm/hooks.read-package-hook'
import { arrayOfWorkspacePackagesToMap } from '@pnpm/installing.context'
import { checkPatchedDepPaths, getLockfileImporterId, type LockfileObject } from '@pnpm/lockfile.fs'
import {
  calcPatchHashes,
  createOverridesMapFromParsed,
  getOutdatedLockfileSetting,
  resolvePatchedDependencies,
} from '@pnpm/lockfile.settings-checker'
import { catalogResolutionsAreUpToDate, getWorkspacePackagesByDirectory, linkedPackagesAreUpToDate, satisfiesPackageManifest } from '@pnpm/lockfile.verification'
import type { WorkspacePackages } from '@pnpm/resolving.resolver-base'
import type { DependencyManifest, Project, ProjectId, ProjectManifest, ReadPackageHook } from '@pnpm/types'
import { clone, once } from 'ramda'

import type { CheckDepsStatusOptions } from './types.js'

export interface AssertWantedLockfileUpToDateContext {
  autoInstallPeers?: boolean
  config: CheckDepsStatusOptions
  excludeLinksFromLockfile?: boolean
  injectWorkspacePackages?: boolean
  linkWorkspacePackages: boolean | 'deep'
  getManifestsByDir: () => Record<string, DependencyManifest>
  getWorkspacePackages: () => WorkspacePackages | undefined
  hookedManifests?: Map<string, Promise<DependencyManifest | undefined>>
  parsedOverrides?: VersionOverride[]
  readPackageHook?: ReadPackageHook | null
  rootDir: string
  patchedDependencies?: Record<string, string>
}

export interface AssertWantedLockfileUpToDateOptions {
  projectDir: string
  projectId: ProjectId
  projectManifest: ProjectManifest
  wantedLockfile: LockfileObject
  wantedLockfileDir: string
}

export function createDepsStatusReadPackageHook (
  config: CheckDepsStatusOptions,
  wantedLockfileDir: string,
  parsedOverrides: VersionOverride[]
): ReadPackageHook | undefined {
  return createReadPackageHook({
    ignoreCompatibilityDb: config.ignoreCompatibilityDb,
    lockfileDir: wantedLockfileDir,
    overrides: parsedOverrides,
    packageExtensions: config.packageExtensions,
    readPackageHook: config.hooks?.readPackage,
    ignoredOptionalDependencies: config.ignoredOptionalDependencies,
  })
}

export async function assertWantedLockfileUpToDate (
  ctx: AssertWantedLockfileUpToDateContext,
  opts: AssertWantedLockfileUpToDateOptions
): Promise<void> {
  const parsedOverrides = ctx.parsedOverrides ?? parseOverrides(ctx.config.overrides ?? {}, ctx.config.catalogs)
  await assertLockfileSettingsUpToDate(ctx.config, opts, parsedOverrides)
  assertPatchHashesUpToDate(opts)

  const readPackageHook = ctx.readPackageHook !== undefined
    ? (ctx.readPackageHook ?? undefined)
    : createDepsStatusReadPackageHook(ctx.config, opts.wantedLockfileDir, parsedOverrides)
  const projectManifest = await readPackageHook?.(clone(opts.projectManifest), opts.projectDir) ?? opts.projectManifest

  const importer = opts.wantedLockfile.importers[opts.projectId]
  if (
    !satisfiesPackageManifest(
      {
        autoInstallPeers: ctx.autoInstallPeers,
        excludeLinksFromLockfile: ctx.excludeLinksFromLockfile,
        ignoredOptionalDependencies: ctx.config.ignoredOptionalDependencies,
      },
      importer,
      projectManifest
    ).satisfies ||
    !catalogResolutionsAreUpToDate(importer, opts.wantedLockfile.catalogs)
  ) {
    throw new PnpmError('RUN_CHECK_DEPS_UNSATISFIED_PKG_MANIFEST', `The lockfile in ${opts.wantedLockfileDir} does not satisfy project of id ${opts.projectId}`, {
      hint: 'Run `pnpm install` to update the lockfile',
    })
  }

  await assertLinkedPackagesUpToDate(ctx, { ...opts, projectManifest }, readPackageHook)
}

export async function assertWorkspaceProjectsUpToDate (
  ctx: {
    allProjects: Project[]
    lockfileDir: string
    opts: CheckDepsStatusOptions
  },
  projectsToCheck: Project[],
  readWantedLockfileAndDir: (projectDir: string) => Promise<{
    wantedLockfile: LockfileObject
    wantedLockfileDir: string
  }>
): Promise<void> {
  const { lockfileDir, opts } = ctx
  const getProjectId = opts.sharedWorkspaceLockfile !== false
    ? (project: Pick<Project, 'rootDir'>) => getLockfileImporterId(lockfileDir, project.rootDir)
    : () => '.' as ProjectId
  const assertContextOf = createProjectAssertContextResolver(ctx)

  await Promise.all(projectsToCheck.map(async (project) => {
    const { wantedLockfile, wantedLockfileDir } = await readWantedLockfileAndDir(project.rootDir)
    await assertWantedLockfileUpToDate(assertContextOf(project), {
      projectDir: project.rootDir,
      projectId: getProjectId(project),
      projectManifest: project.manifest,
      wantedLockfile,
      wantedLockfileDir,
    })
  }))
}

function createProjectAssertContextResolver (
  { allProjects, lockfileDir, opts }: {
    allProjects: Project[]
    lockfileDir: string
    opts: CheckDepsStatusOptions
  }
): (project: Project) => AssertWantedLockfileUpToDateContext {
  const getWorkspacePackages = once(arrayOfWorkspacePackagesToMap.bind(null, allProjects))
  const getManifestsByDir = once(() => getWorkspacePackagesByDirectory(getWorkspacePackages()))
  const baseCtx: AssertWantedLockfileUpToDateContext = {
    autoInstallPeers: opts.autoInstallPeers,
    injectWorkspacePackages: opts.injectWorkspacePackages,
    config: opts,
    excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
    linkWorkspacePackages: opts.linkWorkspacePackages,
    getManifestsByDir,
    getWorkspacePackages,
    rootDir: lockfileDir,
  }
  if (opts.sharedWorkspaceLockfile !== false) {
    const parsedOverrides = parseOverrides(opts.overrides ?? {}, opts.catalogs)
    const sharedCtx: AssertWantedLockfileUpToDateContext = {
      ...baseCtx,
      hookedManifests: new Map(),
      parsedOverrides,
      readPackageHook: createDepsStatusReadPackageHook(opts, lockfileDir, parsedOverrides) ?? null,
    }
    return () => sharedCtx
  }
  const projectConfigs = createProjectConfigRecord(opts)
  return (project) => {
    const projectOverrides = project.manifest.name == null ? undefined : projectConfigs?.[project.manifest.name]?.overrides
    return {
      ...baseCtx,
      config: projectOverrides == null ? opts : { ...opts, overrides: projectOverrides },
      hookedManifests: new Map(),
    }
  }
}

async function assertLockfileSettingsUpToDate (
  config: CheckDepsStatusOptions,
  { wantedLockfile, wantedLockfileDir }: AssertWantedLockfileUpToDateOptions,
  parsedOverrides: VersionOverride[]
): Promise<void> {
  const resolvedPatchedDeps = resolvePatchedDependencies(config.patchedDependencies, wantedLockfileDir)
  const [
    patchedDependencies,
    pnpmfileChecksum,
  ] = await Promise.all([
    calcPatchHashes(resolvedPatchedDeps ?? {}),
    config.hooks?.calculatePnpmfileChecksum?.(),
  ])

  const outdatedLockfileSettingName = getOutdatedLockfileSetting(wantedLockfile, {
    catalogs: config.catalogs,
    autoInstallPeers: config.autoInstallPeers,
    injectWorkspacePackages: config.injectWorkspacePackages,
    excludeLinksFromLockfile: config.excludeLinksFromLockfile,
    peersSuffixMaxLength: config.peersSuffixMaxLength,
    overrides: createOverridesMapFromParsed(parsedOverrides),
    ignoredOptionalDependencies: config.ignoredOptionalDependencies == null ? undefined : [...config.ignoredOptionalDependencies].sort(),
    packageExtensionsChecksum: hashObjectNullableWithPrefix(config.packageExtensions),
    patchedDependencies,
    pnpmfileChecksum,
    ignorePnpmfileChecksum: config.ignorePnpmfile === true && pnpmfileChecksum == null,
  })

  if (outdatedLockfileSettingName) {
    throw new PnpmError('RUN_CHECK_DEPS_OUTDATED_LOCKFILE', `Setting ${outdatedLockfileSettingName} of lockfile in ${wantedLockfileDir} is outdated`, {
      hint: 'Run `pnpm install` to update the lockfile',
    })
  }
}

function assertPatchHashesUpToDate ({ wantedLockfile, wantedLockfileDir }: AssertWantedLockfileUpToDateOptions): void {
  switch (checkPatchedDepPaths(wantedLockfile)) {
    case 'stale':
      throw new PnpmError('RUN_CHECK_DEPS_STALE_PATCH_HASHES', `The lockfile in ${wantedLockfileDir} has patch hashes that disagree with its own "patchedDependencies"`, {
        hint: 'Run `pnpm install` to update the lockfile',
      })
    case 'indeterminate':
      throw new PnpmError('RUN_CHECK_DEPS_UNCHECKABLE_PATCH_HASHES', `The lockfile in ${wantedLockfileDir} cannot be checked for stale patch hashes`, {
        hint: 'Run `pnpm install` to update the lockfile',
      })
    case 'up-to-date':
      break
  }
}

async function assertLinkedPackagesUpToDate (
  ctx: AssertWantedLockfileUpToDateContext,
  { projectDir, projectId, projectManifest, wantedLockfile, wantedLockfileDir }: AssertWantedLockfileUpToDateOptions,
  readPackageHook?: ReadPackageHook
): Promise<void> {
  if (!await linkedPackagesAreUpToDate({
    linkWorkspacePackages: !!ctx.linkWorkspacePackages,
    lockfileDir: wantedLockfileDir,
    workspaceDir: ctx.config.workspaceDir,
    manifestsByDir: ctx.getManifestsByDir(),
    workspacePackages: ctx.getWorkspacePackages(),
    lockfilePackages: wantedLockfile.packages,
    injectWorkspacePackages: ctx.injectWorkspacePackages ?? wantedLockfile.settings?.injectWorkspacePackages,
    resolution: {
      autoInstallPeers: ctx.autoInstallPeers,
      catalogs: ctx.config.catalogs,
      readPackageHook,
    },
    hookedManifests: ctx.hookedManifests,
  }, {
    dir: projectDir,
    manifest: projectManifest,
    snapshot: wantedLockfile.importers[projectId],
  })) {
    throw new PnpmError('RUN_CHECK_DEPS_LINKED_PKGS_OUTDATED', `The linked packages by ${projectDir} is outdated`, {
      hint: 'Run `pnpm install` to update the packages',
    })
  }
}

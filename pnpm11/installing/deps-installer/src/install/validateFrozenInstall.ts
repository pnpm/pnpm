import path from 'node:path'

import { createAllowBuildFunction } from '@pnpm/building.policy'
import { LOCKFILE_MAJOR_VERSION, LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { getContext, type PnpmContext } from '@pnpm/installing.context'
import { createLockfileObject, getWantedLockfileName, type LockfileObject } from '@pnpm/lockfile.fs'
import type { ProjectRootDir } from '@pnpm/types'
import { clone, equals } from 'ramda'

import { extendOptions, type ProcessedInstallOptions as StrictInstallOptions } from './extendInstallOptions.js'
import { allMutationsAreInstalls, isCheckOnlyInstall, pkgHasDependencies } from './installPredicates.js'
import { assertFrozenPnprPnpmfileChecksum, assertPnprCompatibleWithStore } from './installViaPnprServer.js'
import { detectOutdatedLockfileSettings, readInstallChecksums } from './lockfileState.js'
import {
  createScriptsOptions,
  readMutationProjectOptions,
  runPreResolutionHooks,
} from './mutateModules.js'
import type {
  MutatedProject,
  MutateModulesOptions,
  MutationRun,
  PreparedFrozenMutation,
  PreparedFrozenProjectHook,
} from './mutationTypes.js'
import { canUsePnprForMutations, pnpmfileHookPnprCannotRun } from './pnpr.js'
import { assertProjectManifestsExist } from './preparedFrozen.js'
import { requiresFullResolution, throwOnInconsistentFrozenPatchHashes } from './runMutations.js'
import { createOutdatedLockfileMajorVersionError } from './tryFrozenInstall.js'
import {
  checkProjectsAgainstLockfile,
  createNoLockfileError,
  verifyLockedTarballIntegrity,
} from './verifyFrozenLockfile.js'
import { verifyLockfileResolutions } from './verifyLockfileResolutions.js'

export interface ValidatedFrozenInstall {
  wantedLockfile?: LockfileObject
  prepared?: PreparedFrozenMutation
}

/**
 * Runs every read-only check a `--frozen-lockfile` install enforces before
 * `pnpm ci` cleans `node_modules`, and returns the hooked manifests and
 * verification state so the subsequent install runs each hook once.
 */
export async function validateFrozenInstall (
  projects: MutatedProject[],
  maybeOpts: MutateModulesOptions
): Promise<ValidatedFrozenInstall | undefined> {
  const opts = extendOptions({
    ...maybeOpts,
    frozenLockfile: true,
    frozenLockfileIfExists: false,
  })
  if (isCheckOnlyInstall(opts)) return undefined
  const usesPnpr = Boolean(maybeOpts.allProjects?.length) &&
    Boolean(opts.pnprServer) &&
    canUsePnprForMutations(projects, opts) &&
    pnpmfileHookPnprCannotRun(opts.hooks) == null
  if (usesPnpr) {
    assertPnprCompatibleWithStore(opts)
  }
  if (!maybeOpts.allProjects?.length) {
    opts.allProjects = await readMutationProjectOptions(projects, opts.ignorePackageManifest)
  }
  const rawCtx = await getContext({ ...opts, frozenStore: true, readPackageHook: undefined })
  assertProjectManifestsExist(rawCtx, Boolean(maybeOpts.ignorePackageManifest))
  if (opts.fixLockfile) {
    await validateFixLockfilePreflight(rawCtx, { maybeOpts, opts, projects, usesPnpr })
    return undefined
  }
  const cleansVirtualStore = maybeOpts._cleansVirtualStore !== false
  const validationCtx = { cleansVirtualStore, maybeOpts, opts, projects, usesPnpr }
  if (!rawCtx.existsWantedLockfile) {
    return validateWithoutWantedLockfile(rawCtx, validationCtx)
  }
  if (rawCtx.lockfileHadConflicts) {
    return prepareUnverifiedLockfile(rawCtx, validationCtx)
  }
  return verifyFrozenMutationRun(rawCtx, validationCtx)
}

async function validateFixLockfilePreflight (
  rawCtx: PnpmContext,
  { maybeOpts, opts, projects, usesPnpr }: {
    maybeOpts: MutateModulesOptions
    opts: StrictInstallOptions
    projects: MutatedProject[]
    usesPnpr: boolean
  }
): Promise<void> {
  if (!rawCtx.existsWantedLockfile || rawCtx.lockfileHadConflicts) return
  await verifyLockfileStructureAndSettings(rawCtx, { maybeOpts, opts, projects, usesPnpr, checkMajorVersion: false })
  if (!opts.trustLockfile) {
    await verifyLockfilePolicies(rawCtx, opts, { wantedMutated: false })
  }
}

interface FrozenValidationContext {
  cleansVirtualStore: boolean
  maybeOpts: MutateModulesOptions
  opts: StrictInstallOptions
  projects: MutatedProject[]
  usesPnpr: boolean
}

async function validateWithoutWantedLockfile (
  rawCtx: PnpmContext,
  { cleansVirtualStore, maybeOpts, opts, projects, usesPnpr }: FrozenValidationContext
): Promise<ValidatedFrozenInstall | undefined> {
  if (Object.values(rawCtx.projects).some((project) => pkgHasDependencies(project.manifest))) {
    throw createNoLockfileError()
  }
  await verifyLockfileStructureAndSettings(rawCtx, { maybeOpts, opts, projects, usesPnpr, checkMajorVersion: false })
  const hookedProjects = await hookContextProjectsOnce(rawCtx, opts)
  if (Object.values(rawCtx.projects).some((project) => pkgHasDependencies(project.manifest))) {
    throw createNoLockfileError()
  }
  const preResolution = await runPreResolutionForValidation(rawCtx, opts, cleansVirtualStore)
  if (preResolution.wantedMutated) {
    await verifyLockfileStructureAndSettings(rawCtx, { maybeOpts, opts, projects, usesPnpr, checkMajorVersion: false })
  }
  if (!opts.trustLockfile) {
    await verifyLockfilePolicies(rawCtx, opts, { wantedMutated: preResolution.wantedMutated })
  }
  return {
    wantedLockfile: rawCtx.wantedLockfile,
    prepared: {
      existsWantedLockfile: false,
      hookedCurrentLockfile: preResolution.hookedCurrentLockfile,
      hookedProjects,
      hookedWantedLockfile: preResolution.hookedWantedLockfile,
      lockfileHadConflicts: false,
      preResolutionRan: true,
      verifiedLockfile: !opts.trustLockfile,
    },
  }
}

async function prepareUnverifiedLockfile (
  rawCtx: PnpmContext,
  { cleansVirtualStore, maybeOpts, opts, projects, usesPnpr }: FrozenValidationContext
): Promise<ValidatedFrozenInstall> {
  await verifyLockfileStructureAndSettings(rawCtx, { maybeOpts, opts, projects, usesPnpr, checkMajorVersion: false })
  const hookedProjects = await hookContextProjectsOnce(rawCtx, opts)
  const validatedInputLockfile = clone(rawCtx.wantedLockfile)
  const preResolution = await runPreResolutionForValidation(rawCtx, opts, cleansVirtualStore)
  if (preResolution.wantedMutated) {
    await verifyLockfileStructureAndSettings(rawCtx, { maybeOpts, opts, projects, usesPnpr, checkMajorVersion: false })
  }
  if (!opts.trustLockfile) {
    await verifyLockfilePolicies(rawCtx, opts, { wantedMutated: preResolution.wantedMutated })
  }
  return {
    wantedLockfile: rawCtx.wantedLockfile,
    prepared: {
      existsWantedLockfile: rawCtx.existsWantedLockfile,
      hookedCurrentLockfile: preResolution.hookedCurrentLockfile,
      hookedProjects,
      hookedWantedLockfile: preResolution.hookedWantedLockfile,
      lockfileHadConflicts: rawCtx.lockfileHadConflicts,
      preResolutionRan: true,
      validatedInputLockfile,
      verifiedLockfile: !opts.trustLockfile,
    },
  }
}

async function verifyLockfileStructureAndSettings (
  ctx: PnpmContext,
  { maybeOpts, opts, projects, usesPnpr, checkMajorVersion }: {
    maybeOpts: MutateModulesOptions
    opts: StrictInstallOptions
    projects: MutatedProject[]
    usesPnpr: boolean
    checkMajorVersion: boolean
  }
): Promise<void> {
  await verifyLockfileResolutions(ctx.wantedLockfile, [])
  const checksums = await readInstallChecksums({ ctx, opts })
  if (usesPnpr) {
    assertFrozenPnprPnpmfileChecksum(opts, ctx.wantedLockfile.pnpmfileChecksum, checksums.pnpmfileChecksum)
  }
  const settings = detectOutdatedLockfileSettings({ ctx, opts }, checksums)
  throwOnInconsistentFrozenPatchHashes(ctx, true)
  if (!checkMajorVersion) return
  const installsOnly = allMutationsAreInstalls(projects)
  const run = buildValidationRun(ctx, { installsOnly, maybeOpts, opts, projects })
  const upToDateLockfileMajorVersion = ctx.wantedLockfile.lockfileVersion
    .toString()
    .startsWith(`${LOCKFILE_MAJOR_VERSION}.`)
  if (requiresFullResolution(run, {
    outdatedLockfileSettings: settings.changedLockfileSettings.length > 0,
    upToDateLockfileMajorVersion,
  })) {
    throw createOutdatedLockfileMajorVersionError()
  }
}

async function hookContextProjectsOnce (
  ctx: PnpmContext,
  opts: StrictInstallOptions
): Promise<Map<ProjectRootDir, PreparedFrozenProjectHook>> {
  const hookedProjects = new Map<ProjectRootDir, PreparedFrozenProjectHook>()
  const readPackageHook = opts.readPackageHook
  await Promise.all(Object.values(ctx.projects).map(async (project) => {
    if (readPackageHook != null && !project.alreadyHooked) {
      project.originalManifest = project.manifest
      project.manifest = await readPackageHook(clone(project.manifest), project.rootDir)
      project.alreadyHooked = true
    }
    hookedProjects.set(project.rootDir, {
      manifest: project.manifest,
      originalManifest: project.originalManifest,
    })
  }))
  return hookedProjects
}

async function reopenMergedLockfileContextIfNeeded (
  rawCtx: PnpmContext,
  opts: StrictInstallOptions,
  hookedProjects: Map<ProjectRootDir, PreparedFrozenProjectHook>
): Promise<PnpmContext> {
  if (!opts.mergeGitBranchLockfiles || opts.readPackageHook == null) {
    return rawCtx
  }
  const hookedAllProjects = opts.allProjects.map((project) => {
    const hooked = hookedProjects.get(project.rootDir)
    return hooked == null
      ? project
      : { ...project, ...hooked, alreadyHooked: true }
  })
  return getContext({ ...opts, allProjects: hookedAllProjects, frozenStore: true })
}

async function verifyFrozenMutationRun (
  rawCtx: PnpmContext,
  { cleansVirtualStore, maybeOpts, opts, projects, usesPnpr }: {
    cleansVirtualStore: boolean
    maybeOpts: MutateModulesOptions
    opts: StrictInstallOptions
    projects: MutatedProject[]
    usesPnpr: boolean
  }
): Promise<ValidatedFrozenInstall> {
  const structureOpts = { maybeOpts, opts, projects, usesPnpr, checkMajorVersion: true }
  await verifyLockfileStructureAndSettings(rawCtx, structureOpts)
  const hookedProjects = await hookContextProjectsOnce(rawCtx, opts)
  const ctx = await reopenMergedLockfileContextIfNeeded(rawCtx, opts, hookedProjects)
  const validatedInputLockfile = clone(ctx.wantedLockfile)
  const preResolution = await runPreResolutionForValidation(ctx, opts, cleansVirtualStore)
  if (preResolution.wantedMutated || ctx !== rawCtx) {
    await verifyLockfileStructureAndSettings(ctx, structureOpts)
  }
  const installsOnly = allMutationsAreInstalls(projects)
  const run = buildValidationRun(ctx, { installsOnly, maybeOpts, opts, projects })
  if (!opts.ignorePackageManifest) {
    await checkProjectsAgainstLockfile(run, true)
  }
  if (!opts.lockfileOnly) {
    await verifyLockedTarballIntegrity(run)
  }
  if (!opts.trustLockfile) {
    await verifyLockfilePolicies(ctx, opts, { wantedMutated: preResolution.wantedMutated })
  }
  return {
    wantedLockfile: ctx.wantedLockfile,
    prepared: {
      existsWantedLockfile: ctx.existsWantedLockfile,
      hookedCurrentLockfile: preResolution.hookedCurrentLockfile,
      hookedProjects,
      hookedWantedLockfile: preResolution.hookedWantedLockfile,
      lockfileHadConflicts: ctx.lockfileHadConflicts,
      preResolutionRan: true,
      validatedInputLockfile,
      verifiedLockfile: !opts.trustLockfile,
    },
  }
}

interface PreparedPreResolutionLockfiles {
  hookedCurrentLockfile?: LockfileObject
  hookedWantedLockfile?: LockfileObject
  wantedMutated: boolean
}

async function runPreResolutionForValidation (
  ctx: PnpmContext,
  opts: StrictInstallOptions,
  cleansVirtualStore: boolean
): Promise<PreparedPreResolutionLockfiles> {
  if (!opts.hooks.preResolution?.length) return { wantedMutated: false }
  const wantedBefore = clone(ctx.wantedLockfile)
  const importerIds = Object.values(ctx.projects).map(({ id }) => id)
  const currentLockfile = cleansVirtualStore
    ? createLockfileObject(importerIds, {
      autoInstallPeers: opts.autoInstallPeers,
      excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
      lockfileVersion: LOCKFILE_VERSION,
      peersSuffixMaxLength: opts.peersSuffixMaxLength,
    })
    : clone(ctx.currentLockfile)
  const hookCtx: PnpmContext = {
    ...ctx,
    currentLockfile,
    existsCurrentLockfile: cleansVirtualStore ? false : ctx.existsCurrentLockfile,
  }
  await runPreResolutionHooks(opts, hookCtx)
  ctx.wantedLockfile = hookCtx.wantedLockfile
  return {
    hookedCurrentLockfile: hookCtx.currentLockfile,
    hookedWantedLockfile: hookCtx.wantedLockfile,
    wantedMutated: !equals(wantedBefore, hookCtx.wantedLockfile),
  }
}

async function verifyLockfilePolicies (
  ctx: PnpmContext,
  opts: StrictInstallOptions,
  { wantedMutated }: { wantedMutated: boolean }
): Promise<void> {
  const canUseOnDiskCache = !wantedMutated &&
    !opts.mergeGitBranchLockfiles &&
    ctx.existsWantedLockfile &&
    !ctx.lockfileHadConflicts &&
    opts.cacheDir != null &&
    opts.resolutionVerifiers.length > 0
  const wantedLockfileName = canUseOnDiskCache
    ? await getWantedLockfileName({
      useGitBranchLockfile: opts.useGitBranchLockfile,
      mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
    })
    : undefined
  const wantedLockfilePath = wantedLockfileName != null &&
    (!opts.useGitBranchLockfile || wantedLockfileName !== WANTED_LOCKFILE)
    ? path.resolve(ctx.lockfileDir, wantedLockfileName)
    : undefined
  await verifyLockfileResolutions(ctx.wantedLockfile, opts.resolutionVerifiers, {
    cacheDir: wantedMutated ? undefined : opts.cacheDir,
    lockfilePath: wantedLockfilePath,
  })
}

function buildValidationRun (
  ctx: PnpmContext,
  { installsOnly, maybeOpts, opts, projects }: {
    installsOnly: boolean
    maybeOpts: MutateModulesOptions
    opts: MutationRun['opts']
    projects: MutatedProject[]
  }
): MutationRun {
  return {
    allowBuild: createAllowBuildFunction(opts),
    ctx,
    detachReporter: () => {},
    forceResolutionFromHook: false,
    installsAndUninstallsOnly: true,
    installsOnly,
    maybeOpts,
    opts,
    projects,
    pruneVirtualStore: false,
    rootProjectPreinstallRan: false,
    scriptsOpts: createScriptsOptions(opts, ctx),
  }
}

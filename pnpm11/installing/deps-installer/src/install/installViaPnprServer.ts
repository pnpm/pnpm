import path from 'node:path'

import { toRegistryDeclarations } from '@pnpm/config.normalize-registries'
import { PnpmError } from '@pnpm/error'
import { makeProjectNodePathOption, runLifecycleHook } from '@pnpm/exec.lifecycle'
import type { RangeSpecStyle } from '@pnpm/installing.deps-resolver'
import { headlessInstall, type InstallationResultStats } from '@pnpm/installing.deps-restorer'
import {
  convertToLockfileObject,
  getLockfileImporterId,
  isEmptyLockfile,
  type LockfileFile,
  type LockfileObject,
  readWantedLockfileFile,
} from '@pnpm/lockfile.fs'
import { calcPatchHashes, resolvePatchedDependencies } from '@pnpm/lockfile.settings-checker'
import { logger } from '@pnpm/logger'
import { groupPatchedDependenciesWithPaths } from '@pnpm/patching.config'
import type { PnprProject, ResolveViaPnprServerOptions, ResolveViaPnprServerResult } from '@pnpm/pnpr.client'
import type { DepPath, ProjectId, ProjectManifest, ProjectRootDir } from '@pnpm/types'
import { safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'
import { pathAbsolute } from 'path-absolute'

import { LockfileConfigMismatchError } from './frozenInstallErrors.js'
import { materializeOrDelegate } from './installInContext.js'
import {
  DEV_PREINSTALL,
  getUntrackedPnpmfileReadPackageHook,
  installRunsDevPreinstall,
  setUntrackedPnpmfileReadPackageHook,
} from './installPredicates.js'
import type { InstallResult, MutatedProject, SingleProjectInstallOptions } from './mutationTypes.js'
import { applyResolvedSpecsFromLockfile, type PnprNewDep } from './pnpr.js'
import { writeWantedLockfileAndRecordVerified } from './writeWantedLockfileAndRecordVerified.js'

interface PnprInstallProjectOptions {
  rootDir: ProjectRootDir
  manifest: ProjectManifest
  modulesDir?: string
  binsDir?: string
  mutation?: MutatedProject['mutation']
  newDeps?: PnprNewDep[]
  rangeSpecStyle?: RangeSpecStyle
}

interface InstallViaPnprServerArgs {
  manifest: ProjectManifest
  rootDir: ProjectRootDir
  opts: SingleProjectInstallOptions
  allInstallProjects?: PnprInstallProjectOptions[]
  rootProjectPreinstallRan: boolean
}

type PnprInstallResult = InstallResult & { stats: InstallationResultStats, lockfile: LockfileObject }

interface PnprRequestContext extends InstallViaPnprServerArgs {
  lockfileDir: string
  pnprAuthorization: string | undefined
  resolveViaPnprServer: (opts: ResolveViaPnprServerOptions) => Promise<ResolveViaPnprServerResult>
}

/**
 * When a pnpr server is configured, resolve dependencies server-side,
 * then run a headless install that fetches tarballs from the registries
 * and links packages into node_modules — like a normal install.
 */
export async function installViaPnprServer (args: InstallViaPnprServerArgs): Promise<PnprInstallResult> {
  const { opts, rootDir } = args
  // The pnpr server path re-resolves and persists new `index.db` entries plus a
  // freshly written lockfile, so it inherently writes the store. `frozenStore`
  // promises the store is complete and read-only, so the two are mutually
  // exclusive — and the unconditional pnpr gate means this path runs even under
  // `--offline --frozen-lockfile`, so refuse up front with guidance instead of
  // crashing later on the read-only `index.db` open.
  if (opts.frozenStore) {
    throw new PnpmError(
      'FROZEN_STORE_INCOMPATIBLE_WITH_PNPR',
      'The pnpr server resolves dependencies and writes new entries into the store, which is opened read-only when frozenStore is enabled.',
      { hint: 'Disable the pnpr server (unset `--pnpr-server` / `pnprServer` in pnpm-workspace.yaml) so the install reads from the existing store, or unset `frozenStore` to allow store writes.' }
    )
  }
  const { resolveViaPnprServer } = await import('@pnpm/pnpr.client')
  const { createGetAuthHeaderByURI } = await import('@pnpm/network.auth-header')

  // Identify the caller to pnpr's gate. The client does not forward its
  // upstream registry credentials: pnpr selects upstream credentials from
  // its own route policy, so they never travel in the request body.
  const configByUri = opts.configByUri ?? {}
  const pnprAuthorization = createGetAuthHeaderByURI(configByUri)(opts.pnprServer!)

  try {
    return await resolveAndInstallViaPnpr({
      ...args,
      lockfileDir: opts.lockfileDir ?? rootDir,
      pnprAuthorization,
      resolveViaPnprServer,
    })
  } finally {
    // Close the storeController to flush queued StoreIndex writes — the
    // normal install path does the same; skipping it here would leave
    // pending writes on disk and diverge from lifecycle expectations.
    await opts.storeController.close()
  }
}

async function resolveAndInstallViaPnpr (request: PnprRequestContext): Promise<PnprInstallResult> {
  const { lockfileDir, manifest, opts } = request
  // Read the existing lockfile (if any) in its on-disk shape — that's
  // what the pnpr server protocol carries, so no conversion is needed before
  // sending it.
  const existingLockfile = await readWantedLockfileFile(lockfileDir, {
    ignoreIncompatible: true,
  }).catch(() => null)
  const resolvedPatchedDependencies = resolvePatchedDependencies(opts.patchedDependencies, lockfileDir)
  const patchedDependencies = opts.ignorePackageManifest
    ? existingLockfile?.patchedDependencies
    : resolvedPatchedDependencies == null
      ? undefined
      : await calcPatchHashes(resolvedPatchedDependencies)
  const patchGroups = groupPatchedDependenciesWithPaths(
    patchedDependencies,
    resolvedPatchedDependencies
  )

  await runRootProjectHooks(request)

  const lockfile = await resolveLockfileViaPnpr(request, { existingLockfile, patchedDependencies })

  // `--lockfile-only`: the pnpr server resolved and we wrote the lockfile, but
  // pnpm fetches nothing and links nothing in this mode — stop before the
  // headless install. See https://github.com/pnpm/pnpm/issues/12146.
  if (opts.lockfileOnly) {
    return {
      updatedCatalogs: undefined,
      updatedManifest: manifest,
      ignoredBuilds: undefined,
      stats: { added: 0, removed: 0, linkedToRoot: 0 },
      lockfile,
      resolutionPolicyViolations: [],
    }
  }
  return materializePnprLockfile(request, { lockfile, patchGroups })
}

/**
 * The root project's hooks run before the resolution is requested and
 * before the lockfile is written, as on the local resolution path.
 */
async function runRootProjectHooks (
  { allInstallProjects, lockfileDir, manifest, opts, rootDir, rootProjectPreinstallRan }: PnprRequestContext
): Promise<void> {
  const rootProjectManifest = (allInstallProjects ?? [{ rootDir, manifest }])
    .find((project) => project.rootDir === lockfileDir)?.manifest ??
    await safeReadProjectManifestOnly(lockfileDir)
  const rootModulesDir = path.join(lockfileDir, opts.modulesDir ?? 'node_modules')
  const rootHookOpts = {
    depPath: lockfileDir,
    extraBinPaths: opts.extraBinPaths,
    extraEnv: {
      ...opts.extraEnv,
      ...await makeProjectNodePathOption({ modulesDir: rootModulesDir, rootDir: lockfileDir }, opts),
    },
    pkgRoot: lockfileDir,
    rootModulesDir,
    wdBinDir: path.join(rootModulesDir, '.bin'),
    scriptShell: opts.scriptShell,
    scriptsPrependNodePath: opts.scriptsPrependNodePath,
    shellEmulator: opts.shellEmulator,
    stdio: opts.ownLifecycleHooksStdio,
    unsafePerm: opts.unsafePerm || false,
    userAgent: opts.userAgent,
  }
  if (installRunsDevPreinstall(opts) && rootProjectManifest?.scripts?.[DEV_PREINSTALL]) {
    await runLifecycleHook(DEV_PREINSTALL, rootProjectManifest, rootHookOpts)
  }
  if (rootProjectPreinstallRan && rootProjectManifest?.scripts?.preinstall) {
    await runLifecycleHook('preinstall', rootProjectManifest, rootHookOpts)
  }
}

async function resolveLockfileViaPnpr (
  request: PnprRequestContext,
  { existingLockfile, patchedDependencies }: { existingLockfile: LockfileFile | null, patchedDependencies: Record<string, string> | undefined }
): Promise<LockfileObject> {
  const { allInstallProjects, lockfileDir, opts, rootDir } = request
  // Like the local install, `frozenLockfileIfExists` ignores a lockfile
  // that records no dependencies.
  const frozenLockfile = opts.frozenLockfile === true || (
    opts.frozenLockfileIfExists === true &&
    existingLockfile != null &&
    recordsDependencies(existingLockfile)
  )
  const pnpmfileChecksum = await opts.hooks?.calculatePnpmfileChecksum?.()
  // The server skips the pnpmfile comparison a local frozen install makes,
  // and a frozen install must not rewrite the recorded checksum.
  if (frozenLockfile && !opts.ignorePnpmfile && existingLockfile != null && existingLockfile.pnpmfileChecksum !== pnpmfileChecksum) {
    throw new LockfileConfigMismatchError('pnpmfileChecksum')
  }

  logger.info({ message: 'Resolving dependencies via the pnpr server', prefix: rootDir })

  const { lockfile, stats: pnprStats } = await request.resolveViaPnprServer(
    createResolveRequest(request, { existingLockfile, frozenLockfile, patchedDependencies })
  )

  // The server never sees the pnpmfile, so the fields a local resolution
  // records for it are stamped here.
  if (!frozenLockfile) {
    lockfile.pnpmfileChecksum = pnpmfileChecksum
    setUntrackedPnpmfileReadPackageHook(lockfile, getUntrackedPnpmfileReadPackageHook(opts.hooks ?? {}))
  }

  await writeWantedLockfileAndRecordVerified({
    lockfileDir,
    lockfile,
    cacheDir: opts.cacheDir,
    resolutionVerifiers: opts.resolutionVerifiers,
    useGitBranchLockfile: opts.useGitBranchLockfile,
    mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
  })

  if (allInstallProjects) {
    applyResolvedSpecsToAddedProjects(allInstallProjects, { lockfile, lockfileDir })
  }

  logger.info({
    message: `Resolved ${pnprStats.totalPackages} packages`,
    prefix: rootDir,
  })
  return lockfile
}

/**
 * Whether any importer of an on-disk lockfile records a dependency. Only the
 * importers are converted, so the check does not scale with the package count.
 */
function recordsDependencies (lockfile: LockfileFile): boolean {
  return !isEmptyLockfile(convertToLockfileObject({
    lockfileVersion: lockfile.lockfileVersion,
    importers: lockfile.importers,
  }))
}

function createResolveRequest (
  { allInstallProjects, lockfileDir, manifest, opts, pnprAuthorization }: PnprRequestContext,
  { existingLockfile, frozenLockfile, patchedDependencies }: {
    existingLockfile: LockfileFile | null
    frozenLockfile: boolean
    patchedDependencies: Record<string, string> | undefined
  }
): ResolveViaPnprServerOptions {
  const projectsList = listWorkspaceProjects(allInstallProjects, lockfileDir)
  const singleProject = projectsList ? undefined : manifest
  return {
    registryUrl: opts.pnprServer!,
    name: singleProject?.name,
    version: singleProject?.version,
    publishConfig: singleProject && describePublishConfig(singleProject),
    dependencies: singleProject?.dependencies,
    devDependencies: singleProject?.devDependencies,
    optionalDependencies: singleProject?.optionalDependencies,
    peerDependencies: singleProject?.peerDependencies,
    projects: projectsList,
    registry: opts.registriesByScope?.default,
    registries: toRegistryDeclarations(opts),
    authorization: pnprAuthorization,
    overrides: opts.overrides,
    patchedDependencies,
    packageExtensions: opts.packageExtensions,
    allowUnusedPatches: opts.allowUnusedPatches,
    // The reconstructed workspace the server builds from this request has no
    // catalog sections, so forward the catalogs for the server to resolve
    // `catalog:` specifiers in both dependencies and overrides.
    catalogs: opts.catalogs,
    autoInstallPeers: opts.autoInstallPeers,
    dedupePeers: opts.dedupePeers,
    excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
    resolutionMode: opts.resolutionMode,
    minimumReleaseAge: opts.minimumReleaseAge,
    minimumReleaseAgeExclude: opts.minimumReleaseAgeExclude,
    minimumReleaseAgeIgnoreMissingTime: opts.minimumReleaseAgeIgnoreMissingTime,
    trustPolicy: opts.trustPolicy,
    trustPolicyExclude: opts.trustPolicyExclude,
    trustPolicyIgnoreAfter: opts.trustPolicyIgnoreAfter,
    trustLockfile: opts.trustLockfile,
    // Lockfile reuse. Without these the server always reuse-and-updates,
    // so `--frozen-lockfile` would silently resolve and rewrite the very
    // lockfile it promises to leave alone.
    frozenLockfile,
    preferFrozenLockfile: opts.preferFrozenLockfile,
    updatePatches: opts.updatePatches,
    lockfile: existingLockfile ?? undefined,
  }
}

/**
 * The workspace projects of the request, when there is more than one.
 * Separators are normalized to POSIX — on Windows `path.relative` returns
 * backslashes, which the pnpr server rejects (it treats `\` as an
 * unsafe/YAML-injection character and normalizes paths as POSIX).
 */
function listWorkspaceProjects (
  allInstallProjects: PnprInstallProjectOptions[] | undefined,
  lockfileDir: string
): PnprProject[] | undefined {
  if (!allInstallProjects || allInstallProjects.length <= 1) return undefined
  return allInstallProjects.map(project => ({
    dir: (path.relative(lockfileDir, project.rootDir) || '.').split(path.sep).join('/'),
    name: project.manifest.name,
    version: project.manifest.version,
    publishConfig: describePublishConfig(project.manifest),
    dependencies: project.manifest.dependencies,
    devDependencies: project.manifest.devDependencies,
    optionalDependencies: project.manifest.optionalDependencies,
    peerDependencies: project.manifest.peerDependencies,
  }))
}

function describePublishConfig (manifest: ProjectManifest): PnprProject['publishConfig'] {
  return manifest.publishConfig?.directory == null
    ? undefined
    : {
      directory: manifest.publishConfig.directory,
      linkDirectory: manifest.publishConfig.linkDirectory,
    }
}

function applyResolvedSpecsToAddedProjects (
  allInstallProjects: PnprInstallProjectOptions[],
  { lockfile, lockfileDir }: { lockfile: LockfileObject, lockfileDir: string }
): void {
  for (const project of allInstallProjects) {
    if (project.mutation !== 'installSome' || !project.newDeps || project.newDeps.length === 0) continue
    const relative = path.relative(lockfileDir, project.rootDir).split(path.sep).join('/')
    const importerId = (relative || '.') as ProjectId
    const snapshot = lockfile?.importers?.[importerId]
    project.manifest = applyResolvedSpecsFromLockfile(project.manifest, snapshot, project.newDeps, project.rangeSpecStyle)
  }
}

/**
 * The pnpr server only resolves; it serves no file content. Fetch every
 * tarball from the registries with the regular store controller, in
 * parallel, exactly like a normal install. See
 * https://github.com/pnpm/pnpm/issues/12230.
 */
async function materializePnprLockfile (
  request: PnprRequestContext,
  { lockfile, patchGroups }: { lockfile: LockfileObject, patchGroups: ReturnType<typeof groupPatchedDependenciesWithPaths> }
): Promise<PnprInstallResult> {
  const { allInstallProjects, manifest, opts, rootDir, rootProjectPreinstallRan } = request
  const headlessOpts = {
    ...createHeadlessOptions(request),
    patchedDependencies: patchGroups,
    wantedLockfile: lockfile,
  }
  if (opts.beforeLifecycleScripts) {
    const updatedProjects = (allInstallProjects ?? [{ rootDir, manifest }]).map((project) => ({
      manifest: project.manifest,
      rootDir: project.rootDir,
    }))
    await opts.beforeLifecycleScripts({
      updatedProjects,
      updatedCatalogs: undefined,
      newLockfile: lockfile,
      resolutionPolicyViolations: [],
    })
  }
  const { ignoredBuilds, stats } = await materializeOrDelegate(
    { ...opts, rootProjectPreinstallRan },
    // eslint-disable-next-line @typescript-eslint/no-explicit-any -- these install options leave settings such as registriesByScope optional, while HeadlessOptions requires them
    () => headlessInstall(headlessOpts as any)
  )

  return {
    updatedCatalogs: undefined,
    updatedManifest: manifest,
    ignoredBuilds,
    // Pacquet doesn't surface a structured stats return; default to
    // zeros so the pnpr server's non-optional `stats` slot is filled.
    stats: stats ?? { added: 0, removed: 0, linkedToRoot: 0 },
    lockfile,
    // The pnpr server enforces the whole verification policy itself and
    // reports any violation as a terminal `violations` frame, which the
    // client turns into a thrown error — so a resolve that got this far
    // produced a policy-clean lockfile with nothing left to react to.
    resolutionPolicyViolations: [],
  }
}

function createHeadlessOptions ({ allInstallProjects, lockfileDir, manifest, opts, rootDir, rootProjectPreinstallRan }: PnprRequestContext) {
  return {
    ...opts,
    dir: rootDir as string,
    lockfileDir,
    engineStrict: opts.engineStrict ?? false,
    ignoreScripts: opts.ignoreScripts ?? false,
    sideEffectsCacheRead: opts.sideEffectsCacheRead ?? false,
    sideEffectsCacheWrite: opts.sideEffectsCacheWrite ?? false,
    symlink: opts.symlink ?? true,
    enableModulesDir: opts.enableModulesDir ?? true,
    include: opts.include ?? { dependencies: true, devDependencies: true, optionalDependencies: true },
    currentEngine: {
      nodeVersion: opts.nodeVersion,
      pnpmVersion: opts.packageManager?.version ?? '',
    },
    rootProjectPreinstallRan,
    selectedProjectDirs: (allInstallProjects ?? [{ rootDir }]).map(project => project.rootDir),
    allProjects: Object.fromEntries(
      (allInstallProjects ?? [{ rootDir, manifest, binsDir: opts.binsDir }]).map((project, index) => {
        const modulesDir = pathAbsolute(project.modulesDir ?? opts.modulesDir ?? 'node_modules', project.rootDir)
        return [project.rootDir, {
          binsDir: project.binsDir ?? path.join(modulesDir, '.bin'),
          buildIndex: index,
          id: getLockfileImporterId(lockfileDir, project.rootDir),
          manifest: project.manifest,
          modulesDir,
          rootDir: project.rootDir,
        }]
      })
    ),
    hoistedDependencies: {},
    pendingBuilds: [] as string[],
    skipped: new Set<DepPath>(),
  }
}

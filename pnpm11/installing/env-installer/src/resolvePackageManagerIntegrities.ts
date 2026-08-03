import { convertToLockfileFile, createEnvLockfile, readEnvLockfile } from '@pnpm/lockfile.fs'
import { pruneSharedLockfile } from '@pnpm/lockfile.pruner'
import type { EnvLockfile, LockfileObject } from '@pnpm/lockfile.types'
import type { StoreController } from '@pnpm/store.controller'
import type { DepPath, ProjectId, Registries } from '@pnpm/types'
import semver from 'semver'

import { convertToLockfileEnvObject } from './pruneEnvLockfile.js'
import { resolveManifestDependencies } from './resolveManifestDependencies.js'
import { writeVerifiedEnvLockfile } from './writeVerifiedEnvLockfile.js'

const PACKAGE_MANAGER_DEPS_WITH_EXE = ['pnpm', '@pnpm/exe'] as const
const PACKAGE_MANAGER_DEPS_PNPM_ONLY = ['pnpm'] as const
const PNPM_EXE_INTRODUCED = '6.17.1'

export interface ResolvePackageManagerIntegritiesOpts {
  envLockfile?: EnvLockfile
  registries: Registries
  rootDir: string
  storeController: StoreController
  storeDir: string
  /**
   * Whether to read from and write to the env lockfile file on disk.
   * When false, resolution happens purely in memory; callers can still use
   * the returned `EnvLockfile` to perform installs without persisting the
   * resolved pnpm integrity info. Defaults to true.
   */
  save?: boolean
}

/**
 * Checks if the wanted pnpm version integrities are already fully resolved in the env lockfile.
 */
export function isPackageManagerResolved (
  envLockfile: EnvLockfile | undefined,
  pnpmVersion: string
): boolean {
  if (!envLockfile) return false

  const pmDeps = envLockfile.importers['.'].packageManagerDependencies
  if (pmDeps == null) return false
  const wantedDeps = packageManagerDeps(pnpmVersion)
  return Object.keys(pmDeps).length === wantedDeps.length &&
    wantedDeps.every((name) => pmDeps[name]?.version === pnpmVersion)
}

/**
 * The packages the env lockfile pins for `pnpmVersion`.
 *
 * Both the JS `pnpm` and the native `@pnpm/exe` are pinned for the majors that
 * publish the two separately, because the pin is shared and teammates may run
 * either one. Outside that range only `pnpm` exists: before 6.17.1 `@pnpm/exe`
 * was not published yet, and from v12 the unscoped `pnpm` is itself the native
 * executable.
 */
function packageManagerDeps (pnpmVersion: string): readonly string[] {
  const parsed = semver.parse(pnpmVersion, { loose: true })
  if (parsed == null) return PACKAGE_MANAGER_DEPS_WITH_EXE
  if (parsed.major >= 12) return PACKAGE_MANAGER_DEPS_PNPM_ONLY
  // Prereleases of a version that has `@pnpm/exe` ship it too, so compare on
  // the release triple alone.
  return semver.gte(`${parsed.major}.${parsed.minor}.${parsed.patch}`, PNPM_EXE_INTRODUCED)
    ? PACKAGE_MANAGER_DEPS_WITH_EXE
    : PACKAGE_MANAGER_DEPS_PNPM_ONLY
}

/**
 * Resolves integrity checksums for the pnpm packages of the wanted version
 * (see {@link packageManagerDeps}) and their dependencies by calling
 * resolveManifestDependencies. When `opts.save` is true (the default) the
 * results are written to the `packageManagerDependencies` section of
 * `pnpm-lock.yaml`; when false, resolution happens purely in memory and the
 * returned `EnvLockfile` is never persisted to disk.
 */
export async function resolvePackageManagerIntegrities (
  pnpmVersion: string,
  opts: ResolvePackageManagerIntegritiesOpts
): Promise<EnvLockfile> {
  const save = opts.save ?? true
  const envLockfile = opts.envLockfile ?? (save ? await readEnvLockfile(opts.rootDir) : undefined) ?? createEnvLockfile()

  if (isPackageManagerResolved(envLockfile, pnpmVersion)) {
    return envLockfile
  }

  const lockfile = await resolveWantedPnpmPackages(pnpmVersion, opts)

  if (lockfile.packages) {
    // Build packageManagerDependencies from the resolved lockfile importers
    const importer = lockfile.importers['.' as ProjectId]
    const packageManagerDependencies: Record<string, { specifier: string, version: string }> = {}
    for (const [name, version] of Object.entries(importer.dependencies ?? {})) {
      packageManagerDependencies[name] = {
        specifier: importer.specifiers[name],
        version,
      }
    }
    envLockfile.importers['.'].packageManagerDependencies = packageManagerDependencies

    // Merge new packages into the env lockfile object, then prune stale entries
    const merged = convertToLockfileEnvObject(envLockfile)
    for (const [depPath, pkg] of Object.entries(lockfile.packages)) {
      merged.packages![depPath as DepPath] = pkg
    }
    const pruned = pruneSharedLockfile(merged)
    const prunedFile = convertToLockfileFile(pruned)
    envLockfile.packages = prunedFile.packages ?? {}
    envLockfile.snapshots = prunedFile.snapshots ?? {}

    if (save) {
      await writeVerifiedEnvLockfile(opts.rootDir, envLockfile)
    }
  }
  return envLockfile
}

/**
 * Resolves the pnpm packages wanted by `spec`, which may be a range or a
 * dist-tag. `pnpm` alone is resolved first because which packages are wanted
 * (see {@link packageManagerDeps}) is only known once the spec has been
 * resolved to an exact version.
 */
async function resolveWantedPnpmPackages (
  spec: string,
  opts: ResolvePackageManagerIntegritiesOpts
): Promise<LockfileObject> {
  const resolveOpts = {
    dir: opts.rootDir,
    registries: opts.registries,
    storeController: opts.storeController,
    storeDir: opts.storeDir,
  }
  const lockfile = await resolveManifestDependencies({ dependencies: { pnpm: spec } }, resolveOpts)
  const resolvedVersion = lockfile.importers['.' as ProjectId]?.dependencies?.['pnpm']
  if (resolvedVersion == null || !packageManagerDeps(resolvedVersion).includes('@pnpm/exe')) {
    return lockfile
  }
  return resolveManifestDependencies(
    {
      dependencies: {
        'pnpm': spec,
        '@pnpm/exe': spec,
      },
    },
    resolveOpts
  )
}

import { parseRegistryQualifiedVersion } from '@pnpm/deps.path'
import { convertToLockfileFile, createEnvLockfile, readEnvLockfile } from '@pnpm/lockfile.fs'
import { pruneSharedLockfile } from '@pnpm/lockfile.pruner'
import type { EnvLockfile, LockfileObject } from '@pnpm/lockfile.types'
import type { StoreController } from '@pnpm/store.controller'
import type { DepPath, ProjectId, RegistriesByScope } from '@pnpm/types'
import semver from 'semver'

import { convertToLockfileEnvObject } from './pruneEnvLockfile.js'
import { resolveManifestDependencies } from './resolveManifestDependencies.js'
import { writeVerifiedEnvLockfile } from './writeVerifiedEnvLockfile.js'

const PACKAGE_MANAGER_DEPS_WITH_EXE = ['pnpm', '@pnpm/exe'] as const
const PACKAGE_MANAGER_DEPS_PNPM_ONLY = ['pnpm'] as const
const PNPM_EXE_INTRODUCED = '6.17.1'

export interface ResolvePackageManagerIntegritiesOpts {
  envLockfile?: EnvLockfile
  registriesByScope: RegistriesByScope
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
 * `>=6.17.1 <12` publishes the JS `pnpm` and the native `@pnpm/exe` as two
 * packages, and both are pinned, because the pin is shared and teammates may
 * run either one. Every other version publishes `pnpm` alone: as the JS CLI
 * below 6.17.1, and as the native executable itself from 12.
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
  stripRegistryTarballUrls(lockfile)

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
 * Rewrites registry tarball resolutions to integrity-only form, dropping
 * tarball URLs that a registry advertises on a host other than its own —
 * load-balanced proxies and Artifactory-style mirrors do this, see
 * https://github.com/pnpm/pnpm/issues/13619. The package-manager bootstrap
 * never fetches a URL recorded in the lockfile: the download URL is always
 * derived from the trusted bootstrap registries at install time, so a
 * repository-provided entry cannot steer the download. Dropping the URL here
 * keeps freshly resolved entries in exactly the integrity-only shape the
 * bootstrap validation accepts.
 */
function stripRegistryTarballUrls (lockfile: LockfileObject): void {
  for (const pkg of Object.values(lockfile.packages ?? {})) {
    const resolution = pkg.resolution
    if (
      resolution == null ||
      !('integrity' in resolution) || !resolution.integrity ||
      !('tarball' in resolution) || typeof resolution.tarball !== 'string' ||
      resolution.tarball.startsWith('file:') ||
      ('gitHosted' in resolution && resolution.gitHosted === true) ||
      ('path' in resolution && resolution.path != null)
    ) {
      continue
    }
    pkg.resolution = { integrity: resolution.integrity }
  }
}

/**
 * Resolves the pnpm packages wanted by `spec`, which may also be a range or a
 * dist-tag. A spec that is not an exact version is resolved through `pnpm`
 * alone first, because which packages are wanted (see
 * {@link packageManagerDeps}) is only known once the version is known.
 */
async function resolveWantedPnpmPackages (
  spec: string,
  opts: ResolvePackageManagerIntegritiesOpts
): Promise<LockfileObject> {
  const resolveOpts = {
    dir: opts.rootDir,
    registriesByScope: opts.registriesByScope,
    storeController: opts.storeController,
    storeDir: opts.storeDir,
  }
  if (semver.valid(spec) != null) {
    return resolveManifestDependencies({ dependencies: wantedDependencies(spec, spec) }, resolveOpts)
  }
  const lockfile = await resolveManifestDependencies({ dependencies: { pnpm: spec } }, resolveOpts)
  const ref = lockfile.importers['.' as ProjectId]?.dependencies?.['pnpm']
  // A pnpm resolved from a named registry is referenced as `<registry>:<version>`.
  const resolvedVersion = ref == null ? undefined : parseRegistryQualifiedVersion(ref)?.version ?? ref
  if (resolvedVersion == null || !packageManagerDeps(resolvedVersion).includes('@pnpm/exe')) {
    return lockfile
  }
  return resolveManifestDependencies({ dependencies: wantedDependencies(resolvedVersion, spec) }, resolveOpts)
}

/** The dependency map to resolve `spec` through, for a pnpm of `pnpmVersion`. */
function wantedDependencies (pnpmVersion: string, spec: string): Record<string, string> {
  return Object.fromEntries(packageManagerDeps(pnpmVersion).map((name) => [name, spec]))
}

import { parse as parseDepPath, refToRelative, removeSuffix } from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import type { EnvLockfile, LockfilePackageInfo, LockfileResolution } from '@pnpm/lockfile.types'
import type { DepPath } from '@pnpm/types'

export function assertPackageManagerLockfileUsesRegistryResolutions (envLockfile: EnvLockfile): void {
  const packageManagerDependencies = envLockfile.importers['.'].packageManagerDependencies
  if (packageManagerDependencies == null) {
    throw new PnpmError('INVALID_PACKAGE_MANAGER_LOCKFILE', 'The packageManager dependencies were not found in pnpm-lock.yaml')
  }

  const visited = new Set<DepPath>()
  for (const [name, dependency] of Object.entries(packageManagerDependencies)) {
    const depPath = refToRelative(dependency.version, name)
    if (depPath == null) {
      throw invalidPackageManagerLockfile(name)
    }
    assertRegistryPackageManagerDependency(envLockfile, depPath, visited)
  }
}

function assertRegistryPackageManagerDependency (
  envLockfile: EnvLockfile,
  depPath: DepPath,
  visited: Set<DepPath>
): void {
  if (visited.has(depPath)) return
  visited.add(depPath)

  const packageInfo = envLockfile.packages[removeSuffix(depPath)]
  const snapshot = envLockfile.snapshots[depPath]
  if (packageInfo == null || snapshot == null) {
    throw invalidPackageManagerLockfile(depPath)
  }

  assertRegistryPackagePath(depPath, packageInfo)
  assertIntegrityOnlyResolution(depPath, packageInfo.resolution)

  for (const [name, ref] of Object.entries({
    ...snapshot.dependencies,
    ...snapshot.optionalDependencies,
  })) {
    const nextDepPath = refToRelative(ref, name)
    if (nextDepPath == null) {
      throw invalidPackageManagerLockfile(depPath)
    }
    assertRegistryPackageManagerDependency(envLockfile, nextDepPath, visited)
  }
}

function assertRegistryPackagePath (depPath: DepPath, packageInfo: LockfilePackageInfo): void {
  const parsedDepPath = parseDepPath(depPath)
  if (parsedDepPath.name == null || parsedDepPath.version == null || parsedDepPath.nonSemverVersion != null) {
    throw invalidPackageManagerLockfile(depPath)
  }
  if (packageInfo.id != null) {
    throw invalidPackageManagerLockfile(depPath)
  }
  if (packageInfo.name != null && packageInfo.name !== parsedDepPath.name) {
    throw invalidPackageManagerLockfile(depPath)
  }
  if (packageInfo.version != null && packageInfo.version !== parsedDepPath.version) {
    throw invalidPackageManagerLockfile(depPath)
  }
}

function assertIntegrityOnlyResolution (depPath: DepPath, resolution: LockfileResolution): void {
  if (resolution == null || typeof resolution !== 'object' || Array.isArray(resolution)) {
    throw invalidPackageManagerLockfile(depPath)
  }
  const resolutionKeys = Object.keys(resolution)
  if (
    resolutionKeys.length !== 1 ||
    resolutionKeys[0] !== 'integrity' ||
    !('integrity' in resolution) ||
    typeof resolution.integrity !== 'string' ||
    resolution.integrity.length === 0
  ) {
    throw invalidPackageManagerLockfile(depPath)
  }
}

function invalidPackageManagerLockfile (depPath: string): PnpmError {
  return new PnpmError(
    'INVALID_PACKAGE_MANAGER_LOCKFILE',
    `The packageManager dependency "${depPath}" in pnpm-lock.yaml must use a registry package path and an integrity-only resolution`
  )
}

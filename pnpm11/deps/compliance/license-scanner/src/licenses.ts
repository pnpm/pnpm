import { PnpmError } from '@pnpm/error'
import { detectDepTypes } from '@pnpm/lockfile.detect-dep-types'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type {
  DependenciesField,
  IncludedDependencies,
  ProjectId,
  ProjectManifest,
  RegistriesByScope,
  SupportedArchitectures,
} from '@pnpm/types'
import semver from 'semver'

import {
  type LicenseNode,
  lockfileToLicenseNodeTree,
} from './lockfileToLicenseNodeTree.js'

export interface LicensePackage {
  belongsTo: DependenciesField
  version: string
  name: string
  /**
   * Named-registry alias the package was resolved from (lockfile format
   * 9.1), or `undefined` for the default/scope registry. Part of the
   * package's identity: the same name and version served by two
   * registries are two distinct artifacts with their own licenses.
   */
  registryName?: string
  license: string
  licenseContents?: string
  author?: string
  homepage?: string
  description?: string
  repository?: string
  path?: string
}

/**
 * @private
 * Returns an array of LicensePackages from the given LicenseNode
 * @param licenseNode the license node
 * @returns LicensePackage[]
 */
function getDependenciesFromLicenseNode (
  licenseNode: LicenseNode
): LicensePackage[] {
  const dependencies: LicensePackage[] = []
  appendDependenciesFromLicenseNode(licenseNode, dependencies)
  return dependencies
}

function appendDependenciesFromLicenseNode (
  licenseNode: LicenseNode,
  dependencies: LicensePackage[]
): void {
  for (const dependencyNode of Object.values(licenseNode.dependencies ?? {})) {
    appendDependenciesFromLicenseNode(dependencyNode, dependencies)
    dependencies.push({
      belongsTo: dependencyNode.dev ? 'devDependencies' : 'dependencies',
      version: dependencyNode.version as string,
      name: dependencyNode.name as string,
      registryName: dependencyNode.registryName,
      license: dependencyNode.license as string,
      licenseContents: dependencyNode.licenseContents,
      author: dependencyNode.author as string,
      homepage: dependencyNode.homepage as string,
      description: dependencyNode.description,
      repository: dependencyNode.repository as string,
      path: dependencyNode.dir,
    })
  }
}

export async function findDependencyLicenses (opts: {
  ignoreDependencies?: Set<string>
  include?: IncludedDependencies
  lockfileDir: string
  manifest: ProjectManifest
  storeDir: string
  virtualStoreDir: string
  virtualStoreDirMaxLength: number
  modulesDir?: string
  registriesByScope: RegistriesByScope
  registriesByPrefix?: Record<string, string>
  wantedLockfile: LockfileObject | null
  includedImporterIds?: ProjectId[]
  supportedArchitectures?: SupportedArchitectures
}): Promise<LicensePackage[]> {
  if (opts.wantedLockfile == null) {
    throw new PnpmError(
      'LICENSES_NO_LOCKFILE',
      `No lockfile in directory "${opts.lockfileDir}". Run \`pnpm install\` to generate one.`
    )
  }

  const depTypes = detectDepTypes(opts.wantedLockfile)
  const licenseNodeTree = await lockfileToLicenseNodeTree(opts.wantedLockfile, {
    dir: opts.lockfileDir,
    modulesDir: opts.modulesDir,
    storeDir: opts.storeDir,
    virtualStoreDir: opts.virtualStoreDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    include: opts.include,
    registriesByScope: opts.registriesByScope,
    registriesByPrefix: opts.registriesByPrefix,
    includedImporterIds: opts.includedImporterIds,
    supportedArchitectures: opts.supportedArchitectures,
    depTypes,
  })

  // map: name@ver (qualified by named registry, when any) -> LicensePackage
  const licensePackages = new Map<string, LicensePackage>()

  for (const dependencyName in licenseNodeTree.dependencies) {
    const licenseNode = licenseNodeTree.dependencies[dependencyName]
    const dependenciesOfNode = getDependenciesFromLicenseNode(licenseNode)

    for (const dependencyNode of dependenciesOfNode) {
      // The registry is part of the identity: the same name and version
      // served by two registries are different artifacts and may carry
      // different licenses, so they must not collapse onto one entry.
      const mapKey = dependencyNode.registryName == null
        ? `${dependencyNode.name}@${dependencyNode.version}`
        : `${dependencyNode.name}@${dependencyNode.registryName}:${dependencyNode.version}`
      const existingVersion = licensePackages.get(mapKey)?.version
      if (existingVersion === undefined) {
        licensePackages.set(mapKey, dependencyNode)
      }
    }
  }

  // Get all non-duplicate dependencies of the project
  const projectDependencies = Array.from(licensePackages.values())
  return Array.from(projectDependencies).sort((pkg1, pkg2) =>
    pkg1.name.localeCompare(pkg2.name) || semver.compare(pkg1.version, pkg2.version)
  )
}

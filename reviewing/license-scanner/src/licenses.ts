import { PnpmError } from '@pnpm/error'
import { type Lockfile } from '@pnpm/lockfile-file'
import { detectDepTypes } from '@pnpm/lockfile.detect-dep-types'
import {
  type SupportedArchitectures,
  type DependenciesField,
  type IncludedDependencies,
  type ProjectManifest,
  type Registries,
} from '@pnpm/types'
import {
  type LicenseNode,
  lockfileToLicenseNodeTree,
} from './lockfileToLicenseNodeTree'
import semver from 'semver'

export interface LicensePackage {
  belongsTo: DependenciesField
  version: string
  name: string
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
  if (!licenseNode.dependencies) {
    return []
  }

  let dependencies: LicensePackage[] = []
  for (const dependencyName in licenseNode.dependencies) {
    const dependencyNode = licenseNode.dependencies[dependencyName]
    const dependenciesOfNode = getDependenciesFromLicenseNode(dependencyNode)

    dependencies = [
      ...dependencies,
      ...dependenciesOfNode,
      {
        belongsTo: dependencyNode.dev ? 'devDependencies' : 'dependencies',
        version: dependencyNode.version as string,
        name: dependencyName,
        license: dependencyNode.license as string,
        licenseContents: dependencyNode.licenseContents,
        author: dependencyNode.author as string,
        homepage: dependencyNode.homepage as string,
        description: dependencyNode.description,
        repository: dependencyNode.repository as string,
        path: dependencyNode.dir,
      },
    ]
  }

  return dependencies
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
  registries: Registries
  wantedLockfile: Lockfile | null
  includedImporterIds?: string[]
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
    registries: opts.registries,
    includedImporterIds: opts.includedImporterIds,
    supportedArchitectures: opts.supportedArchitectures,
    depTypes,
  })

  // map: name@ver -> LicensePackage
  const licensePackages = new Map<string, LicensePackage>()

  for (const dependencyName in licenseNodeTree.dependencies) {
    const licenseNode = licenseNodeTree.dependencies[dependencyName]
    const dependenciesOfNode = getDependenciesFromLicenseNode(licenseNode)

    dependenciesOfNode.forEach((dependencyNode) => {
      const mapKey = `${dependencyNode.name}@${dependencyNode.version}`
      const existingVersion = licensePackages.get(mapKey)?.version
      if (existingVersion === undefined) {
        licensePackages.set(mapKey, dependencyNode)
      }
    })
  }

  // Get all non-duplicate dependencies of the project
  const projectDependencies = Array.from(licensePackages.values())
  return Array.from(projectDependencies).sort((pkg1, pkg2) =>
    pkg1.name.localeCompare(pkg2.name) || semver.compare(pkg1.version, pkg2.version)
  )
}

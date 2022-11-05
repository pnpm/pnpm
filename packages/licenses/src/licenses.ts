import { PnpmError } from '@pnpm/error'
import { Lockfile, PackageSnapshot } from '@pnpm/lockfile-file'
import {
  DependenciesField,
  IncludedDependencies,
  ProjectManifest,
  Registries,
} from '@pnpm/types'
import { getPkgInfo } from './getPkgInfo'
import {
  LicenseNode,
  lockfileToLicenseNodeTree,
} from './lockfileToLicenseNodeTree'

export interface LicensePackage {
  belongsTo: DependenciesField
  version: string
  name: string
  license: string
  licenseContents?: string
  vendorName?: string
  vendorUrl?: string
  vendorRepository?: string
  path?: string
}

export type GetPackageInfoFunction = (
  pkg: {
    name?: string
    version?: string
    depPath: string
    snapshot: PackageSnapshot
    registries: Registries
  },
  opts: {
    storeDir: string
    virtualStoreDir: string
    dir: string
    modulesDir: string
  }
) => Promise<{
  from: string
  description?: string
} & Omit<LicensePackage, 'belongsTo'>>

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
        vendorName: dependencyNode.vendorName as string,
        vendorUrl: dependencyNode.vendorUrl as string,
        vendorRepository: dependencyNode.repository as string,
        path: dependencyNode.dir,
      },
    ]
  }

  return dependencies
}

export async function licences (opts: {
  getPackageInfo?: GetPackageInfoFunction
  ignoreDependencies?: Set<string>
  include?: IncludedDependencies
  lockfileDir: string
  manifest: ProjectManifest
  storeDir: string
  virtualStoreDir: string
  modulesDir?: string
  registries: Registries
  wantedLockfile: Lockfile | null
}): Promise<LicensePackage[]> {
  if (opts.wantedLockfile == null) {
    throw new PnpmError(
      'LICENSES_NO_LOCKFILE',
      `No lockfile in directory "${opts.lockfileDir}". Run \`pnpm install\` to generate one.`
    )
  }

  const licenseNodeTree = await lockfileToLicenseNodeTree(opts.wantedLockfile, {
    dir: opts.lockfileDir,
    modulesDir: opts.modulesDir,
    storeDir: opts.storeDir,
    virtualStoreDir: opts.virtualStoreDir,
    include: opts.include,
    registries: opts.registries,
    getPackageInfo: opts.getPackageInfo ?? getPkgInfo,
  })

  const licensePackages = new Map<string, LicensePackage>()
  for (const dependencyName in licenseNodeTree.dependencies) {
    const licenseNode = licenseNodeTree.dependencies[dependencyName]
    const dependenciesOfNode = getDependenciesFromLicenseNode(licenseNode)

    dependenciesOfNode.forEach((dependencyNode) => {
      licensePackages.set(dependencyNode.name, dependencyNode)
    })
  }

  // Get all non-duplicate dependencies of the project
  const projectDependencies = Array.from(licensePackages.values())
  return Array.from(projectDependencies).sort((pkg1, pkg2) =>
    pkg1.name.localeCompare(pkg2.name)
  )
}

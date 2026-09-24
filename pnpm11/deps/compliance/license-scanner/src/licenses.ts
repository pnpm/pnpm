import path from 'node:path'

import { PnpmError } from '@pnpm/error'
import { readModulesManifest } from '@pnpm/installing.modules-yaml'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type {
  DependenciesField,
  IncludedDependencies,
  ProjectId,
  ProjectManifest,
  RegistriesByScope,
  SupportedArchitectures,
} from '@pnpm/types'

import { compareVersions } from './compareVersions.js'
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
  /** Installed copies retained when several locations share one package version. */
  paths?: string[]
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
      ...(dependencyNode.paths == null ? {} : { paths: dependencyNode.paths }),
    })
  }
}

export async function findDependencyLicenses (opts: {
  ignoreDependencies?: Set<string>
  include?: IncludedDependencies
  dir?: string
  lockfileDir: string
  manifest: ProjectManifest
  storeDir: string
  virtualStoreDir: string
  virtualStoreDirMaxLength: number
  modulesDir?: string
  /**
   * Lockfile-relative directories keyed by dependency path, recorded by a
   * `nodeLinker: hoisted` install, which leaves the virtual store empty.
   */
  hoistedLocations?: Record<string, string[]>
  nodeLinker?: 'hoisted' | 'isolated' | 'pnp'
  shamefullyHoist?: boolean
  registriesByScope: RegistriesByScope
  registriesByPrefix?: Record<string, string>
  wantedLockfile: LockfileObject | null
  includedImporterIds?: ProjectId[]
  resolvePeersFromWorkspaceRoot?: boolean
  supportedArchitectures?: SupportedArchitectures
}): Promise<LicensePackage[]> {
  if (opts.wantedLockfile == null) {
    throw new PnpmError(
      'LICENSES_NO_LOCKFILE',
      `No lockfile in directory "${opts.lockfileDir}". Run \`pnpm install\` to generate one.`
    )
  }

  const modulesDir = opts.modulesDir ?? 'node_modules'
  const rootModulesDir = path.resolve(opts.lockfileDir, modulesDir)
  const projectModulesDir = opts.dir ? path.resolve(opts.dir, modulesDir) : rootModulesDir
  const modulesManifest = await readModulesManifest(rootModulesDir) ??
    (projectModulesDir !== rootModulesDir ? await readModulesManifest(projectModulesDir) : null)

  const nodeLinker = opts.nodeLinker ?? modulesManifest?.nodeLinker
  const shamefullyHoist = opts.shamefullyHoist ?? modulesManifest?.shamefullyHoist ?? Boolean(modulesManifest?.publicHoistPattern?.includes('*'))
  const hoistedLocations = opts.hoistedLocations ?? modulesManifest?.hoistedLocations

  const licenseNodeTree = await lockfileToLicenseNodeTree(opts.wantedLockfile, {
    dir: opts.dir ?? opts.lockfileDir,
    lockfileDir: opts.lockfileDir,
    modulesDir: opts.modulesDir,
    hoistedLocations,
    storeDir: opts.storeDir,
    virtualStoreDir: opts.virtualStoreDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    include: opts.include,
    nodeLinker,
    shamefullyHoist,
    registriesByScope: opts.registriesByScope,
    registriesByPrefix: opts.registriesByPrefix,
    includedImporterIds: opts.includedImporterIds,
    resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
    supportedArchitectures: opts.supportedArchitectures,
  })

  // map: name@ver (qualified by named registry, when any) and license -> LicensePackage
  const licensePackages = new Map<string, LicensePackage>()

  for (const dependencyName in licenseNodeTree.dependencies) {
    const licenseNode = licenseNodeTree.dependencies[dependencyName]
    const dependenciesOfNode = getDependenciesFromLicenseNode(licenseNode)

    for (const dependencyNode of dependenciesOfNode) {
      // The registry is part of the identity: the same name and version
      // served by two registries are different artifacts and may carry
      // different licenses, so they must not collapse onto one entry. Two
      // local packages can share a name and version but not their license.
      const pkgId = dependencyNode.registryName == null
        ? `${dependencyNode.name}@${dependencyNode.version}`
        : `${dependencyNode.name}@${dependencyNode.registryName}:${dependencyNode.version}`
      const mapKey = `${pkgId}\u0000${dependencyNode.license}`
      const existing = licensePackages.get(mapKey)
      if (existing === undefined) {
        licensePackages.set(mapKey, dependencyNode)
      } else {
        mergeLicensePackagePaths(existing, dependencyNode)
      }
    }
  }

  // Get all non-duplicate dependencies of the project
  const projectDependencies = Array.from(licensePackages.values())
  return Array.from(projectDependencies).sort((pkg1, pkg2) =>
    pkg1.name.localeCompare(pkg2.name) || compareVersions(pkg1.version, pkg2.version)
  )
}

/**
 * Adds the installed locations of `added` to `target`, which describes the
 * same package version and license, so that every copy stays in the report.
 * Each side contributes its `paths`, or its `path` when `paths` is unset.
 * `target.paths` is set to the distinct locations only when there are more
 * than one; otherwise `target` is left unchanged.
 */
export function mergeLicensePackagePaths (target: LicensePackage, added: LicensePackage): void {
  const paths = [...new Set([...installedPaths(target), ...installedPaths(added)])]
  if (paths.length > 1) target.paths = paths
}

function installedPaths (pkg: LicensePackage): string[] {
  return pkg.paths ?? (pkg.path ? [pkg.path] : [])
}

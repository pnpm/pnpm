import { packageIsInstallable } from '@pnpm/config.package-is-installable'
import { withCollapsedVariants } from '@pnpm/deps.path'
import { DepType, type DepTypes, detectDepTypes } from '@pnpm/lockfile.detect-dep-types'
import type { LockfileObject, TarballResolution } from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot, packageIdFromSnapshot } from '@pnpm/lockfile.utils'
import {
  lockfileWalkerGroupImporterSteps,
  type LockfileWalkerStep,
} from '@pnpm/lockfile.walker'
import { StoreIndex } from '@pnpm/store.index'
import type { DependenciesField, DepPath, ProjectId, RegistriesByScope, SupportedArchitectures } from '@pnpm/types'

import { getPkgInfo } from './getPkgInfo.js'

export interface LicenseNode {
  name?: string
  version?: string
  /** Named-registry alias when the package came from one; see `LicensePackage.registryName`. */
  registryName?: string
  license: string
  licenseContents?: string
  dir: string
  paths?: string[]
  author?: string
  homepage?: string
  description?: string
  repository?: string
  integrity?: string
  requires?: Record<string, string>
  dependencies?: Record<string, LicenseNode>
  dev: boolean
}

export type LicenseNodeTree = Omit<
  LicenseNode,
'dir' | 'license' | 'licenseContents' | 'author' | 'homepages' | 'repository'
>

export interface LicenseExtractOptions {
  storeDir: string
  storeIndex: StoreIndex
  virtualStoreDir: string
  virtualStoreDirMaxLength: number
  modulesDir?: string
  dir: string
  lockfileDir?: string
  nodeLinker?: 'hoisted' | 'isolated' | 'pnp'
  shamefullyHoist?: boolean
  hoistedLocations?: Record<string, string[]>
  registriesByScope: RegistriesByScope
  registriesByPrefix?: Record<string, string>
  supportedArchitectures?: SupportedArchitectures
  depTypes: DepTypes
}

export async function lockfileToLicenseNode (
  step: LockfileWalkerStep,
  options: LicenseExtractOptions
): Promise<Record<string, LicenseNode>> {
  const entries = await Promise.all(
    step.dependencies.map((dependency) => extractLicenseNode(dependency, options))
  )
  return Object.fromEntries(entries.filter((entry): entry is [string, LicenseNode] => entry != null))
}

async function extractLicenseNode (
  dependency: LockfileWalkerStep['dependencies'][number],
  options: LicenseExtractOptions
): Promise<[string, LicenseNode] | null> {
  const { depPath, pkgSnapshot, next } = dependency
  const { name, version, registryName } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)

  const packageInstallable = packageIsInstallable(pkgSnapshot.id ?? depPath, {
    name,
    version,
    cpu: pkgSnapshot.cpu,
    os: pkgSnapshot.os,
    libc: pkgSnapshot.libc,
  }, {
    optional: pkgSnapshot.optional ?? false,
    lockfileDir: options.lockfileDir ?? options.dir,
    supportedArchitectures: options.supportedArchitectures,
  })
  if (!packageInstallable) return null

  const packageInfo = await fetchPackageInfo(depPath, pkgSnapshot, name, version, options)
  const subdeps = await lockfileToLicenseNode(next(), options)
  const dep: LicenseNode = {
    name,
    registryName,
    dev: options.depTypes[depPath] === DepType.DevOnly,
    integrity: (pkgSnapshot.resolution as TarballResolution).integrity,
    version,
    license: packageInfo.license,
    licenseContents: packageInfo.licenseContents,
    author: packageInfo.author,
    homepage: packageInfo.homepage,
    description: packageInfo.description,
    repository: packageInfo.repository,
    dir: packageInfo.path as string,
    ...(packageInfo.paths == null ? {} : { paths: packageInfo.paths }),
  }
  if (Object.keys(subdeps).length > 0) {
    dep.dependencies = subdeps
    dep.requires = toRequires(subdeps)
  }
  return [depPath, dep]
}

async function fetchPackageInfo (
  depPath: DepPath,
  pkgSnapshot: LockfileWalkerStep['dependencies'][number]['pkgSnapshot'],
  name: string,
  version: string,
  options: LicenseExtractOptions
) {
  return getPkgInfo(
    {
      id: packageIdFromSnapshot(depPath, pkgSnapshot),
      name,
      version,
      depPath,
      snapshot: pkgSnapshot,
      registriesByScope: options.registriesByScope,
      registriesByPrefix: options.registriesByPrefix,
    },
    {
      storeDir: options.storeDir,
      storeIndex: options.storeIndex,
      virtualStoreDir: options.virtualStoreDir,
      virtualStoreDirMaxLength: options.virtualStoreDirMaxLength,
      dir: options.dir,
      lockfileDir: options.lockfileDir ?? options.dir,
      modulesDir: options.modulesDir ?? 'node_modules',
      nodeLinker: options.nodeLinker,
      shamefullyHoist: options.shamefullyHoist,
      hoistedLocations: options.hoistedLocations,
      supportedArchitectures: options.supportedArchitectures,
    }
  )
}

/**
 * Reads the lockfile and converts it in a node tree of information necessary
 * to generate the licenses summary
 * @param lockfile the lockfile to process
 * @param opts     parsing instructions
 * @returns
 */
export async function lockfileToLicenseNodeTree (
  lockfile: LockfileObject,
  opts: {
    include?: { [dependenciesField in DependenciesField]: boolean }
    includedImporterIds?: ProjectId[]
    resolvePeersFromWorkspaceRoot?: boolean
  } & Omit<LicenseExtractOptions, 'storeIndex' | 'depTypes'>
): Promise<LicenseNodeTree> {
  const importerWalkers = lockfileWalkerGroupImporterSteps(
    lockfile,
    opts.includedImporterIds ?? Object.keys(lockfile.importers) as ProjectId[],
    { include: opts.include, resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot }
  )
  const extractOptions: LicenseExtractOptions = {
    ...opts,
    lockfileDir: opts.lockfileDir ?? opts.dir,
    depTypes: detectDepTypes(lockfile, opts),
    hoistedLocations: opts.hoistedLocations && withCollapsedVariants(opts.hoistedLocations),
    storeIndex: new StoreIndex(opts.storeDir),
  }
  const dependencies: Record<string, LicenseNode> = Object.fromEntries(
    await Promise.all(
      importerWalkers.map((importerWalker) => extractImporterLicenseNode(importerWalker, extractOptions))
    )
  )
  extractOptions.storeIndex.close()

  const licenseNodeTree: LicenseNodeTree = {
    name: undefined,
    version: undefined,
    dependencies,
    dev: false,
    integrity: undefined,
    requires: toRequires(dependencies),
  }

  return licenseNodeTree
}

async function extractImporterLicenseNode (
  importerWalker: ReturnType<typeof lockfileWalkerGroupImporterSteps>[number],
  options: LicenseExtractOptions
): Promise<[string, LicenseNode]> {
  const importerDeps = await lockfileToLicenseNode(importerWalker.step, options)
  return [importerWalker.importerId, {
    dependencies: importerDeps,
    requires: toRequires(importerDeps),
    version: '0.0.0',
    license: undefined,
  } as unknown as LicenseNode]
}

function toRequires (licenseNodes: Record<string, LicenseNode>): Record<string, string> {
  return Object.fromEntries(
    Object.entries(licenseNodes)
      .map(([key, licenseNode]) => [licenseNode.name ?? key, licenseNode.version!])
  )
}

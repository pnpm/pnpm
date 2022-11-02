import path from 'path'
import { Lockfile } from '@pnpm/lockfile-types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile-utils'
import {
  lockfileWalkerGroupImporterSteps,
  LockfileWalkerStep,
} from '@pnpm/lockfile-walker'
import { DependenciesField, PackageManifest } from '@pnpm/types'
import * as dp from 'dependency-path'
import { GetPackageInfoFunction } from './licenses'

export interface LicenseNode {
  name?: string
  version?: string
  license: string
  licenseContents?: string
  packageManifest: PackageManifest
  dir: string
  vendorName?: string
  vendorUrl?: string
  integrity?: string
  requires?: Record<string, string>
  dependencies?: { [name: string]: LicenseNode }
  dev: boolean
}

export type LicenseNodeTree = Omit<
LicenseNode,
'dir' | 'license' | 'vendorName' | 'vendorUrl' | 'packageManifest'
>

export interface LicenseExtractOptions {
  virtualStoreDir: string
  modulesDir?: string
  dir: string
  getPackageInfo: GetPackageInfoFunction
}

export async function lockfileToLicenseNode (
  step: LockfileWalkerStep,
  options: LicenseExtractOptions
) {
  const dependencies = {}
  for (const dependency of step.dependencies) {
    const { depPath, pkgSnapshot, next } = dependency
    const { name, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)

    const modules = path.join(
      options.dir,
      options.virtualStoreDir,
      dp.depPathToFilename(depPath),
      options.modulesDir ?? 'node_modules'
    )

    let packageDetails
    try {
      packageDetails = await options.getPackageInfo({
        name,
        version,
        prefix: modules,
      })
    } catch (err: unknown) {}

    if (packageDetails) {
      const subdeps = await lockfileToLicenseNode(next(), options)

      const { packageInfo, packageManifest } = packageDetails
      const dep: LicenseNode = {
        name,
        dev: pkgSnapshot.dev === true,
        integrity: pkgSnapshot.resolution['integrity'],
        version,
        packageManifest,
        license: packageInfo.license,
        licenseContents: packageInfo.licenseContents,
        vendorName: packageInfo.author,
        vendorUrl: packageInfo.homepage,
        dir: packageInfo.path,
      }

      if (Object.keys(subdeps).length > 0) {
        dep.dependencies = subdeps
        dep.requires = toRequires(subdeps)
      }

      // If the package details could be fetched, we consider it part of the tree
      dependencies[name] = dep
    }
  }

  return dependencies
}

/**
 * Reads the lockfile and converts it in a node tree of information necessary
 * to generate the licenses summary
 * @param lockfile the lockfile to process
 * @param opts     parsing instructions
 * @returns
 */
export async function lockfileToLicenseNodeTree (
  lockfile: Lockfile,
  opts: {
    getPackageInfo: GetPackageInfoFunction
    include?: { [dependenciesField in DependenciesField]: boolean }
  } & LicenseExtractOptions
): Promise<LicenseNodeTree> {
  const importerWalkers = lockfileWalkerGroupImporterSteps(
    lockfile,
    Object.keys(lockfile.importers),
    { include: opts?.include }
  )
  const dependencies = {}

  for (const importerWalker of importerWalkers) {
    const importerDeps = await lockfileToLicenseNode(importerWalker.step, {
      virtualStoreDir: opts.virtualStoreDir,
      modulesDir: opts.modulesDir,
      dir: opts.dir,
      getPackageInfo: opts.getPackageInfo,
    })

    const depName = importerWalker.importerId
    dependencies[depName] = {
      dependencies: importerDeps,
      requires: toRequires(importerDeps),
      version: '0.0.0',
    }
  }

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

function toRequires (licenseNodesByDepName: Record<string, LicenseNode>) {
  const requires = {}
  for (const subdepName of Object.keys(licenseNodesByDepName)) {
    requires[subdepName] = licenseNodesByDepName[subdepName].version
  }
  return requires
}

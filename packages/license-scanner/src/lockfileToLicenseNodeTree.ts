import { Lockfile } from '@pnpm/lockfile-types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile-utils'
import { packageIsInstallable } from '@pnpm/package-is-installable'
import {
  lockfileWalkerGroupImporterSteps,
  LockfileWalkerStep,
} from '@pnpm/lockfile-walker'
import { DependenciesField, Registries } from '@pnpm/types'
import { getPkgInfo } from './getPkgInfo'
import mapValues from 'ramda/src/map'

export interface LicenseNode {
  name?: string
  version?: string
  license: string
  licenseContents?: string
  dir: string
  author?: string
  homepage?: string
  repository?: string
  integrity?: string
  requires?: Record<string, string>
  dependencies?: { [name: string]: LicenseNode }
  dev: boolean
}

export type LicenseNodeTree = Omit<
LicenseNode,
'dir' | 'license' | 'licenseContents' | 'author' | 'homepages' | 'repository'
>

export interface LicenseExtractOptions {
  storeDir: string
  virtualStoreDir: string
  modulesDir?: string
  dir: string
  registries: Registries
}

export async function lockfileToLicenseNode (
  step: LockfileWalkerStep,
  options: LicenseExtractOptions
) {
  const dependencies = {}
  for (const dependency of step.dependencies) {
    const { depPath, pkgSnapshot, next } = dependency
    const { name, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)

    const packageInstallable = packageIsInstallable(pkgSnapshot.id ?? depPath, {
      name,
      version,
      cpu: pkgSnapshot.cpu,
      os: pkgSnapshot.os,
      libc: pkgSnapshot.libc,
    }, {
      optional: pkgSnapshot.optional ?? false,
      lockfileDir: options.dir,
    })

    // If the package is not installable on the given platform, we ignore the
    // package, typically the case for platform prebuild packages
    if (!packageInstallable) {
      continue
    }

    const packageInfo = await getPkgInfo(
      {
        name,
        version,
        depPath,
        snapshot: pkgSnapshot,
        registries: options.registries,
      },
      {
        storeDir: options.storeDir,
        virtualStoreDir: options.virtualStoreDir,
        dir: options.dir,
        modulesDir: options.modulesDir ?? 'node_modules',
      }
    )

    const subdeps = await lockfileToLicenseNode(next(), options)

    const dep: LicenseNode = {
      name,
      dev: pkgSnapshot.dev === true,
      integrity: pkgSnapshot.resolution['integrity'],
      version,
      license: packageInfo.license,
      licenseContents: packageInfo.licenseContents,
      author: packageInfo.author,
      homepage: packageInfo.homepage,
      repository: packageInfo.repository,
      dir: packageInfo.path as string,
    }

    if (Object.keys(subdeps).length > 0) {
      dep.dependencies = subdeps
      dep.requires = toRequires(subdeps)
    }

    // If the package details could be fetched, we consider it part of the tree
    dependencies[name] = dep
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
      storeDir: opts.storeDir,
      virtualStoreDir: opts.virtualStoreDir,
      modulesDir: opts.modulesDir,
      dir: opts.dir,
      registries: opts.registries,
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

function toRequires (licenseNodesByDepName: Record<string, LicenseNode>): Record<string, string> {
  return mapValues((licenseNode) => licenseNode.version!, licenseNodesByDepName)
}

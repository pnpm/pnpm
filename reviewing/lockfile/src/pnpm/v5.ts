import type { Lockfile, PackageSnapshot, ProjectSnapshot } from '@pnpm/lockfile-types'
import { DEPENDENCIES_FIELDS } from '@pnpm/types'
import { createBuilder } from '../graph/builder'
import * as utils from './utils'

function convertFromLockfileFileMutable (lockfileFile: any): Lockfile { // eslint-disable-line
  if (typeof lockfileFile.importers === 'undefined') {
    lockfileFile.importers = {
      '.': {
        specifiers: lockfileFile.specifiers ?? {},
        dependenciesMeta: lockfileFile.dependenciesMeta,
        publishDirectory: lockfileFile.publishDirectory,
      },
    }
    delete lockfileFile.specifiers
    for (const depType of DEPENDENCIES_FIELDS) {
      if (lockfileFile[depType] !== null && typeof lockfileFile[depType] !== 'undefined') {
        lockfileFile.importers['.'][depType] = lockfileFile[depType]
        delete lockfileFile[depType]
      }
    }
  }
  return lockfileFile as Lockfile
}

function refToRelative (reference: string, pkgName: string): string {
  if (reference.startsWith('link:')) {
    return reference
  }
  if (
    !reference.includes('/') ||
    (reference.includes('(') && reference.lastIndexOf('/', reference.indexOf('(')) === -1)
  ) {
    return `/${pkgName}/${reference}`
  }
  return reference
}

function entryNodes (projectSnapshot: ProjectSnapshot): string[] {
  return Object.entries({
    ...projectSnapshot.devDependencies,
    ...projectSnapshot.dependencies,
    ...projectSnapshot.optionalDependencies,
  })
    .map(([pkgName, reference]) => refToRelative(reference, pkgName))
}

function nextPackages (currentPackage: PackageSnapshot): string[] {
  return Object.entries({
    ...currentPackage.dependencies,
    ...currentPackage.optionalDependencies,
  })
    .map(([pkgName, reference]) => refToRelative(reference, pkgName))
}

export const graphBuilder = createBuilder<ProjectSnapshot, PackageSnapshot, Lockfile>({
  parse: convertFromLockfileFileMutable,
  refToRelative,
  entryNodes,
  nextPackages,
  parseDependencyPath (dependencyPath) {
    return utils.parseDependencyPath(dependencyPath, {
      splitToParts (depPath) {
        return depPath.split('/')
      },
      parseVersion (version) {
        const peerSepIndex = version.indexOf('_')
        if (peerSepIndex !== -1) {
          return {
            version: version.substring(0, peerSepIndex),
            peersSuffix: version.substring(peerSepIndex + 1),
          }
        }
        return { version }
      },
    })
  },
})

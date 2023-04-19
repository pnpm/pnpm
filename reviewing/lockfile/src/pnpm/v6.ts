import type { Lockfile, PackageSnapshot } from '@pnpm/lockfile-types'
import { DEPENDENCIES_FIELDS } from '@pnpm/types'
import type { DependenciesMeta } from '@pnpm/types'
import { createBuilder } from '../graph/builder'
import * as utils from './utils'

interface InlineSpecifiersProjectSnapshot {
  dependencies?: InlineSpecifiersResolvedDependencies
  devDependencies?: InlineSpecifiersResolvedDependencies
  optionalDependencies?: InlineSpecifiersResolvedDependencies
  dependenciesMeta?: DependenciesMeta
}

interface InlineSpecifiersResolvedDependencies {
  [depName: string]: SpecifierAndResolution
}

interface SpecifierAndResolution {
  specifier: string
  version: string
}

interface InlineSpecifiersLockfile extends Omit<Lockfile, 'lockfileVersion' | 'importers'> {
  lockfileVersion: string
  importers: Record<string, InlineSpecifiersProjectSnapshot>
}

const INLINE_SPECIFIERS_FORMAT_LOCKFILE_VERSION_SUFFIX = '-inlineSpecifiers'

export function isExperimentalInlineSpecifiersFormat (
  lockfile: Omit<Lockfile, 'lockfileVersion'> & { lockfileVersion: string | number }
): boolean {
  const { lockfileVersion } = lockfile
  return lockfileVersion.toString().startsWith('6.') || typeof lockfileVersion === 'string' && lockfileVersion.endsWith(INLINE_SPECIFIERS_FORMAT_LOCKFILE_VERSION_SUFFIX)
}

function convertFromLockfileFileMutable (lockfileFile: any): InlineSpecifiersLockfile { // eslint-disable-line
  if (typeof lockfileFile.importers === 'undefined') {
    lockfileFile.importers = {
      '.': {
        dependenciesMeta: lockfileFile.dependenciesMeta,
      },
    }
    for (const depType of DEPENDENCIES_FIELDS) {
      if (lockfileFile[depType] !== null && typeof lockfileFile[depType] !== 'undefined') {
        lockfileFile.importers['.'][depType] = lockfileFile[depType]
        delete lockfileFile[depType]
      }
    }
  }
  return lockfileFile as InlineSpecifiersLockfile
}

function refToRelative (reference: string, pkgName: string): string {
  if (reference.startsWith('link:')) {
    return reference
  }
  if (!reference.includes('/') || !reference.replace(/(\([^)]+\))+$/, '').includes('/')) {
    return `/${pkgName}@${reference}`
  }
  return reference
}

function entryNodes (projectSnapshot: InlineSpecifiersProjectSnapshot): string[] {
  return Object.entries({
    ...Object.fromEntries(
      Object.entries(projectSnapshot.devDependencies ?? {}).map(([key, inline]) => [key, inline.version])
    ),
    ...Object.fromEntries(
      Object.entries(projectSnapshot.dependencies ?? {}).map(([key, inline]) => [key, inline.version])
    ),
    ...Object.fromEntries(
      Object.entries(projectSnapshot.optionalDependencies ?? {}).map(([key, inline]) => [key, inline.version])
    ),
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

export const graphBuilder = createBuilder<InlineSpecifiersProjectSnapshot, PackageSnapshot, InlineSpecifiersLockfile>({
  parse: convertFromLockfileFileMutable,
  refToRelative,
  entryNodes,
  nextPackages,
  parseDependencyPath (dependencyPath) {
    return utils.parseDependencyPath(dependencyPath, {
      splitToParts (depPath) {
        const [pre, last] = depPath.split('@', 2)
        return [...pre.split('/'), last]
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

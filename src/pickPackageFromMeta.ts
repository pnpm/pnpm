import semver = require('semver')
import { RegistryPackageSpec } from './parsePref'
import {PackageInRegistry, PackageMeta} from './pickPackage'

export default function (
  spec: RegistryPackageSpec,
  preferredVersionSelector: {
    selector: string,
    type: 'version' | 'range' | 'tag',
  } | undefined,
  meta: PackageMeta,
): PackageInRegistry {
  let version: string | undefined
  switch (spec.type) {
    case 'version':
      version = spec.fetchSpec
      break
    case 'tag':
      version = meta['dist-tags'][spec.fetchSpec]
      break
    case 'range':
      version = pickVersionByVersionRange(meta, spec.fetchSpec, preferredVersionSelector)
      break
  }
  return meta.versions[version as string]
}

function pickVersionByVersionRange (
  meta: PackageMeta,
  versionRange: string,
  preferredVerSel?: {
    type: 'version' | 'range' | 'tag',
    selector: string,
  },
) {
  let versions: string[] | undefined
  const latest = meta['dist-tags'].latest

  if (preferredVerSel && preferredVerSel.selector !== versionRange) {
    const preferredVersions: string[] = []
    switch (preferredVerSel.type) {
      case 'tag': {
        preferredVersions.push(meta['dist-tags'][preferredVerSel.selector])
        break
      }
      case 'range': {
        // This might be slow if there are many versions
        // and the package is an indirect dependency many times in the project.
        // If it will create noticable slowdown, then might be a good idea to add some caching
        versions = Object.keys(meta.versions)
        for (const version of versions) {
          if (semver.satisfies(version, preferredVerSel.selector, true)) {
            preferredVersions.push(version)
          }
        }
        break
      }
      case 'version': {
        if (meta.versions[preferredVerSel.selector]) {
          preferredVersions.push(preferredVerSel.selector)
        }
        break
      }
    }

    if (preferredVersions.indexOf(latest) !== -1 && semver.satisfies(latest, versionRange, true)) {
      return latest
    }
    const preferredVersion = semver.maxSatisfying(preferredVersions, versionRange, true)
    if (preferredVersion) {
      return preferredVersion
    }
  }

  // Not using semver.satisfies in case of * because it does not select beta versions.
  // E.g.: 1.0.0-beta.1. See issue: https://github.com/pnpm/pnpm/issues/865
  if (versionRange === '*' || semver.satisfies(latest, versionRange, true)) {
    return latest
  }
  const maxVersion = semver.maxSatisfying(versions || Object.keys(meta.versions), versionRange, true)
  return maxVersion
}

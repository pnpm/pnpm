import type { LockfileResolution } from '@pnpm/lockfile.types'
import type { Resolution } from '@pnpm/resolver-base'
import getNpmTarballUrl from 'get-npm-tarball-url'

export function toLockfileResolution (
  pkg: {
    name: string
    version: string
  },
  resolution: Resolution,
  registry: string,
  lockfileIncludeTarballUrl?: boolean
): LockfileResolution {
  if (resolution.type !== undefined || !resolution['integrity']) {
    return resolution as LockfileResolution
  }
  if (lockfileIncludeTarballUrl) {
    return {
      integrity: resolution['integrity'],
      tarball: resolution['tarball'],
    }
  }
  if (lockfileIncludeTarballUrl === false) {
    return {
      integrity: resolution['integrity'],
    }
  }
  // Sometimes packages are hosted under non-standard tarball URLs.
  // For instance, when they are hosted on npm Enterprise. See https://github.com/pnpm/pnpm/issues/867
  // Or in other weird cases, like https://github.com/pnpm/pnpm/issues/1072
  const expectedTarball = getNpmTarballUrl(pkg.name, pkg.version, { registry })
  const actualTarball = resolution['tarball'].replaceAll('%2f', '/')
  if (removeProtocol(expectedTarball) !== removeProtocol(actualTarball)) {
    return {
      integrity: resolution['integrity'],
      tarball: resolution['tarball'],
    }
  }
  return {
    integrity: resolution['integrity'],
  }
}

function removeProtocol (url: string): string {
  return url.split('://')[1]
}

import path from 'node:path'

import { PACKAGE_ROOT_LINK_PREFIX, packageRootLinkTarget } from '@pnpm/deps.path'

import { isTarballFilename } from './parseBareSpecifier.js'

/**
 * Converts a relative `file:` specifier that a package from a tarball or the
 * registry declares into a `link:<root>/...` reference to a directory inside
 * that package. That package has no directory to resolve the path against
 * until it is placed on disk, so the link is resolved when it is linked.
 *
 * A tarball target, an absolute or home-relative path, the package's own
 * directory, and a path that leaves the package return `undefined`.
 */
export function fileSpecToPackageRootLink (bareSpecifier: string): string | undefined {
  if (!bareSpecifier.startsWith('file:')) return undefined
  const target = bareSpecifier.slice('file:'.length).replaceAll('\\', '/')
  if (target === '' || target.startsWith('/') || target.startsWith('~') || /^[a-z]:/i.test(target) || isTarballFilename(target)) {
    return undefined
  }
  const normalized = path.posix.normalize(target).replace(/\/$/, '')
  if (normalized === '.' || normalized === '..' || normalized.startsWith('../')) return undefined
  const link = `${PACKAGE_ROOT_LINK_PREFIX}${normalized}`
  return packageRootLinkTarget(link) != null ? link : undefined
}

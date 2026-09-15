import path from 'node:path'

import { PnpmError } from '@pnpm/error'
import { valid } from 'semver'
import validateNpmPackageName from 'validate-npm-package-name'

interface PackageIdentity {
  name: string
  version: string
}

interface CreateTarballFilenameOptions extends PackageIdentity {
  suffix?: string
}

export function createTarballFilename ({ name, version, suffix }: CreateTarballFilenameOptions): string {
  validatePackageIdentity({ name, version })

  const filename = `${normalizePackageName(name)}-${version}${suffix == null ? '' : `-${suffix}`}.tgz`
  if (path.basename(filename) !== filename || path.win32.basename(filename) !== filename) {
    throw new PnpmError('INVALID_TARBALL_FILENAME', `Invalid tarball filename "${filename}".`)
  }
  return filename
}

export function validatePackageIdentity ({ name, version }: PackageIdentity): void {
  if (!validateNpmPackageName(name).validForOldPackages) {
    throw new PnpmError('INVALID_PACKAGE_NAME', `Invalid package name "${name}".`)
  }
  if (!valid(version)) {
    throw new PnpmError('INVALID_PACKAGE_VERSION', `Invalid package version "${version}".`)
  }
}

export function normalizePackageName (name: string): string {
  return name.replace('@', '').replace('/', '-')
}

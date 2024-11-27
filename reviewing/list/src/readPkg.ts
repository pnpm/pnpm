import { readPackageJson } from '@pnpm/read-package-json'
import { type PackageManifest } from '@pnpm/types'
import pLimit from 'p-limit'

const limitPkgReads = pLimit(4)

export async function readPkg (pkgPath: string): Promise<PackageManifest> {
  return limitPkgReads(async () => readPackageJson(pkgPath))
}

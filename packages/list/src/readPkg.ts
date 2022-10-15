import { readPackageJson } from '@pnpm/read-package-json'
import pLimit from 'p-limit'

const limitPkgReads = pLimit(4)

export async function readPkg (pkgPath: string) {
  return limitPkgReads(async () => readPackageJson(pkgPath))
}

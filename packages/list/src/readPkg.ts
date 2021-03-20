import readPackageJson from '@pnpm/read-package-json'
import normalizePackageData from 'normalize-package-data'
import pLimit from 'p-limit'

const limitPkgReads = pLimit(4)

export default async (pkgPath: string) => limitPkgReads(
  async () => {
    const pkgJson = await readPackageJson(pkgPath)
    normalizePackageData(pkgJson)
    return pkgJson
  }
)

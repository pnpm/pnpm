import readPackageJson from '@pnpm/read-package-json'
import pLimit from 'p-limit'

const limitPkgReads = pLimit(4)

export default async (pkgPath: string) => limitPkgReads(async () => readPackageJson(pkgPath))

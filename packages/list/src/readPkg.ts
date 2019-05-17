import readPackageJson from '@pnpm/read-package-json'
import pLimit from 'p-limit'

const limitPkgReads = pLimit(4)

export default (pkgPath: string) => limitPkgReads(() => readPackageJson(pkgPath))

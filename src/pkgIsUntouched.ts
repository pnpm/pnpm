import fs = require('mz/fs')
import dirsum from './fs/dirsum'

export default async function untouched (pkgDir: string): Promise<Boolean> {
  const realShasum = await dirsum(pkgDir)
  const originalShasum = await fs.readFile(`${pkgDir}_shasum`, 'utf8')
  return realShasum === originalShasum
}

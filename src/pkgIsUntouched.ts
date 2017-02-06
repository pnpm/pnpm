import fs = require('mz/fs')
import dirsum from './fs/dirsum'

export default async function untouched (pkgDir: string): Promise<Boolean> {
  const realShasum = await dirsum(pkgDir)
  let originalShasum: string | null = null
  try {
    originalShasum = await fs.readFile(`${pkgDir}_shasum`, 'utf8')
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
    return false // for backward compatibility
  }
  return realShasum === originalShasum
}

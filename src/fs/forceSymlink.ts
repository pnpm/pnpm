import fs = require('mz/fs')
import createDebug from '../debug'
const debug = createDebug('pnpm:symlink')

export type SymlinkType = 'junction' | 'dir'

/*
 * Creates a symlink. Re-link if a symlink already exists at the supplied
 * srcPath. API compatible with [`fs#symlink`](https://nodejs.org/api/fs.html#fs_fs_symlink_srcpath_dstpath_type_callback).
 */

export default async function forceSymlink (srcPath: string, dstPath: string, type: SymlinkType) {
  debug(`${srcPath} -> ${dstPath}`)
  try {
    fs.symlinkSync(srcPath, dstPath, type)
    return
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'EEXIST') throw err

    const linkString = await fs.readlink(dstPath)
    if (srcPath === linkString) {
      return
    }
    await fs.unlink(dstPath)
    await forceSymlink(srcPath, dstPath, type)
  }
}

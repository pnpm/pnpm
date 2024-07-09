import assert from 'assert'
import path from 'path'
import util from 'util'
import fs from 'fs'
import { globalWarn } from '@pnpm/logger'

export function hardLinkDir (src: string, destDirs: string[]): void {
  if (destDirs.length === 0) return
  // Don't try to hard link the source directory to itself
  destDirs = destDirs.filter((destDir) => path.relative(destDir, src) !== '')
  _hardLinkDir(src, destDirs, true)
}

function _hardLinkDir (src: string, destDirs: string[], isRoot?: boolean) {
  let files: string[] = []
  try {
    files = fs.readdirSync(src)
  } catch (err: unknown) {
    if (!isRoot || !((util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT'))) throw err
    globalWarn(`Source directory not found when creating hardLinks for: ${src}. Creating destinations as empty: ${destDirs.join(', ')}`)
    for (const dir of destDirs) {
      fs.mkdirSync(dir, { recursive: true })
    }
    return
  }
  for (const file of files) {
    if (file === 'node_modules') continue
    const srcFile = path.join(src, file)
    if (fs.lstatSync(srcFile).isDirectory()) {
      const destSubdirs = destDirs.map((destDir) => {
        const destSubdir = path.join(destDir, file)
        try {
          fs.mkdirSync(destSubdir, { recursive: true })
        } catch (err: unknown) {
          if (!(util.types.isNativeError(err) && 'code' in err && err.code === 'EEXIST')) throw err
        }
        return destSubdir
      })
      _hardLinkDir(srcFile, destSubdirs)
      continue
    }
    for (const destDir of destDirs) {
      const destFile = path.join(destDir, file)
      try {
        linkOrCopyFile(srcFile, destFile)
      } catch (err: unknown) {
        if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
          // Ignore broken symlinks
          continue
        }
        throw err
      }
    }
  }
}

function linkOrCopyFile (srcFile: string, destFile: string): void {
  try {
    linkOrCopy(srcFile, destFile)
  } catch (err: unknown) {
    assert(util.types.isNativeError(err))
    if ('code' in err && err.code === 'ENOENT') {
      fs.mkdirSync(path.dirname(destFile), { recursive: true })
      linkOrCopy(srcFile, destFile)
      return
    }
    if (!('code' in err && err.code === 'EEXIST')) {
      throw err
    }
  }
}

/*
 * This function could be optimized because we don't really need to try linking again
 * if linking failed once.
 */
function linkOrCopy (srcFile: string, destFile: string): void {
  try {
    fs.linkSync(srcFile, destFile)
  } catch (err: unknown) {
    if (!(util.types.isNativeError(err) && 'code' in err && err.code === 'EXDEV')) throw err
    fs.copyFileSync(srcFile, destFile)
  }
}

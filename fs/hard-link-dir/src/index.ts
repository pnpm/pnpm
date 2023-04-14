import path from 'path'
import { promises as fs } from 'fs'
import { globalWarn } from '@pnpm/logger'

export async function hardLinkDir (src: string, destDirs: string[]) {
  if (destDirs.length === 0) return
  // Don't try to hard link the source directory to itself
  destDirs = destDirs.filter((destDir) => path.relative(destDir, src) !== '')
  await _hardLinkDir(src, destDirs, true)
}

async function _hardLinkDir (src: string, destDirs: string[], isRoot?: boolean) {
  let files: string[] = []
  try {
    files = await fs.readdir(src)
  } catch (err: any) { // eslint-disable-line
    if (!isRoot || err.code !== 'ENOENT') throw err
    globalWarn(`Source directory not found when creating hardLinks for: ${src}. Creating destinations as empty: ${destDirs.join(', ')}`)
    await Promise.all(
      destDirs.map((dir) => fs.mkdir(dir, { recursive: true }))
    )
    return
  }
  await Promise.all(
    files.map(async (file) => {
      if (file === 'node_modules') return
      const srcFile = path.join(src, file)
      if ((await fs.lstat(srcFile)).isDirectory()) {
        const destSubdirs = await Promise.all(
          destDirs.map(async (destDir) => {
            const destSubdir = path.join(destDir, file)
            try {
              await fs.mkdir(destSubdir, { recursive: true })
            } catch (err: any) { // eslint-disable-line
              if (err.code !== 'EEXIST') throw err
            }
            return destSubdir
          })
        )
        await _hardLinkDir(srcFile, destSubdirs)
        return
      }
      await Promise.all(
        destDirs.map(async (destDir) => {
          const destFile = path.join(destDir, file)
          try {
            await linkOrCopyFile(srcFile, destFile)
          } catch (err: any) { // eslint-disable-line
            if (err.code === 'ENOENT') {
              // Ignore broken symlinks
              return
            }
            throw err
          }
        })
      )
    })
  )
}

async function linkOrCopyFile (srcFile: string, destFile: string) {
  try {
    await linkOrCopy(srcFile, destFile)
  } catch (err: any) { // eslint-disable-line
    if (err.code === 'ENOENT') {
      await fs.mkdir(path.dirname(destFile), { recursive: true })
      await linkOrCopy(srcFile, destFile)
      return
    }
    if (err.code !== 'EEXIST') {
      throw err
    }
  }
}

/*
 * This function could be optimized because we don't really need to try linking again
 * if linking failed once.
 */
async function linkOrCopy (srcFile: string, destFile: string) {
  try {
    await fs.link(srcFile, destFile)
  } catch (err: any) { // eslint-disable-line
    if (err.code !== 'EXDEV') throw err
    await fs.copyFile(srcFile, destFile)
  }
}

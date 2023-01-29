import path from 'path'
import { promises as fs } from 'fs'

export async function hardLinkDir (src: string, destDirs: string[]) {
  if (destDirs.length === 0) return
  // Don't try to hard link the source directory to itself
  destDirs = destDirs.filter((destDir) => path.relative(destDir, src) !== '')
  const files = await fs.readdir(src)
  await Promise.all(
    files.map(async (file) => {
      if (file === 'node_modules') return
      const srcFile = path.join(src, file)
      if ((await fs.lstat(srcFile)).isDirectory()) {
        await Promise.all(
          destDirs.map(async (destDir) => {
            const destFile = path.join(destDir, file)
            try {
              await fs.mkdir(destFile, { recursive: true })
            } catch (err: any) { // eslint-disable-line
              if (err.code !== 'EEXIST') throw err
            }
            return hardLinkDir(srcFile, [destFile])
          })
        )
        return
      }
      await Promise.all(
        destDirs.map(async (destDir) => {
          const destFile = path.join(destDir, file)
          try {
            await linkOrCopy(srcFile, destFile)
          } catch (err: any) { // eslint-disable-line
            if (err.code === 'ENOENT') {
              await fs.mkdir(destDir, { recursive: true })
              await linkOrCopy(srcFile, destFile)
              return
            }
            if (err.code !== 'EEXIST') {
              throw err
            }
          }
        })
      )
    })
  )
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

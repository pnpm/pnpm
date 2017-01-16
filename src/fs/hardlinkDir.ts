import path = require('path')
import fs = require('mz/fs')
import mkdirp from '../fs/mkdirp'

export default async function hardlinkDir(existingDir: string, newDir: string) {
  await mkdirp(newDir)
  const dirs = await fs.readdir(existingDir)
  await Promise.all(
    dirs
      .map(async (relativePath: string) => {
        const existingPath = path.join(existingDir, relativePath)
        const newPath = path.join(newDir, relativePath)
        const stat = await fs.lstat(existingPath)
        if (stat.isSymbolicLink()) {
          // filter out broken symlinks
          if (!await fs.exists(existingPath)) {
            return;
          }
          return safeLink(existingPath, newPath)
        }
        if (stat.isDirectory()) {
          return hardlinkDir(existingPath, newPath)
        }
        if (stat.isFile()) {
          return safeLink(existingPath, newPath)
        }
      })
  )
}

async function safeLink(existingPath: string, newPath: string) {
  try {
    await fs.link(existingPath, newPath)
  } catch (err) {
    // shouldn't normally happen, but if the file was already somehow linked,
    // the installation should not fail
    if (err.code !== 'EEXIST') throw err
  }
}

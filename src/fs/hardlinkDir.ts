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
    // EEXIST: shouldn't normally happen, but if the file was already somehow linked,
    // the installation should not fail
    // ENOENT: might happen if package contains a broken symlink
    if (err.code !== 'EEXIST' && err.code !== 'ENOENT') throw err
  }
}

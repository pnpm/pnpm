import {Stats} from 'fs';
import path = require('path')
import fs = require('mz/fs')
import mkdirp from '../fs/mkdirp'
import logger from 'pnpm-logger'

export default async function hardlinkDir(existingDir: string, newDir: string) {
  await mkdirp(newDir)
  const dirs = await fs.readdir(existingDir)
  await Promise.all(
    dirs
      .map(async (relativePath: string) => {
        const existingPath = path.join(existingDir, relativePath)
        const newPath = path.join(newDir, relativePath)
        const stat = await fs.lstat(existingPath)
        if (stat.isSymbolicLink() || stat.isFile()) {
          return safeLink(existingPath, newPath, stat)
        }
        if (stat.isDirectory()) {
          return hardlinkDir(existingPath, newPath)
        }
      })
  )
}

async function safeLink(existingPath: string, newPath: string, stat: Stats) {
  try {
    await fs.link(existingPath, newPath)
  } catch (err) {
    // shouldn't normally happen, but if the file was already somehow linked,
    // the installation should not fail
    if (err.code === 'EEXIST') {
      return
    }
    // might happen if package contains a broken symlink, we don't fail on this
    if (err.code === 'ENOENT' && stat.isSymbolicLink()) {
      logger.warn({
        message: `Broken symlink found: ${existingPath}`,
      })
      return
    }
    throw err
  }
}

import fs = require('mz/fs')
import path = require('path')
import flatten = require('arr-flatten')

export default async function (modules: string) {
  const dirs = await getDirectories(modules)
  const subdirs = await Promise.all(
    dirs.map((dir: string): Promise<string[]> => {
      return isScopedPkgsDir(dir) ? getDirectories(dir) : Promise.resolve([dir])
    })
  )
  return flatten(subdirs)
}

async function getDirectories (srcPath: string): Promise<string[]> {
  let dirs: string[]
  try {
    dirs = await fs.readdir(srcPath)
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'ENOENT') {
      throw err
    }
    dirs = []
  }
  return dirs
    .filter(relativePath => relativePath[0] !== '.') // ignore directories like .bin, .store, etc
    .map(relativePath => path.join(srcPath, relativePath))
    .filter(absolutePath => fs.statSync(absolutePath).isDirectory())
}

function isScopedPkgsDir (dirPath: string) {
  return path.basename(dirPath)[0] === '@'
}

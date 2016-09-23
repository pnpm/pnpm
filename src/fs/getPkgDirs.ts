import fs = require('mz/fs')
import path = require('path')

export default function (modules: string) {
  return getDirectories(modules)
    .reduce((pkgDirs: string[], dir: string): string[] => {
        return pkgDirs.concat(isScopedPkgsDir(dir) ? getDirectories(dir) : [dir])
    }, [])
}

function getDirectories (srcPath: string): string[] {
  let dirs: string[]
  try {
    dirs = fs.readdirSync(srcPath)
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

import fs = require('mz/fs')
import {Stats} from 'fs'
import path = require('path')
import linkBins from './link_bins'

export default function linkBundledDeps (root: string) {
  const nodeModules = path.join(root, 'node_modules')

  return isDir(nodeModules, () =>
    Promise.all(fs.readdirSync(nodeModules).map((mod: Stats) =>
      isDir(path.join(nodeModules, mod), () =>
        symlinkBundledDep(nodeModules, path.join(nodeModules, mod))
      )
    )))
}

function symlinkBundledDep (nodeModules: string, submod: string) {
  return linkBins(nodeModules)
}

async function isDir (path: string, fn: () => Promise<any>) {
  try {
    const stat = await fs.stat(path)
    if (!stat.isDirectory()) return
    return fn()
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code !== 'ENOENT') throw err
  }
}

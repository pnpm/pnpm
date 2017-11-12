import logger from '@pnpm/logger'
import path = require('path')
import isInnerLink = require('is-inner-link')
import fs = require('mz/fs')
import mkdirp = require('mkdirp-promise')
import isSubdir = require('is-subdir')

export default async function safeIsInnerLink (
  modules: string,
  depName: string,
  opts: {
    storePath: string,
  }
): Promise<true | string> {
  try {
    const link = await isInnerLink(modules, depName)

    if (link.isInner) return true

    if (isSubdir(opts.storePath, link.target)) return true

    return link.target as string
  } catch (err) {
    if (err.code === 'ENOENT') return true

    logger.warn(`Moving ${depName} that was installed by a different package manager to "node_modules/.ignored`)
    const ignoredDir = path.join(modules, '.ignored', depName)
    await mkdirp(path.dirname(ignoredDir))
    await fs.rename(
      path.join(modules, depName),
      ignoredDir)
    return true
  }
}

import logger from '@pnpm/logger'
import isInnerLink = require('is-inner-link')
import isSubdir = require('is-subdir')
import mkdirp = require('mkdirp-promise')
import fs = require('mz/fs')
import path = require('path')

export default async function safeIsInnerLink (
  modules: string,
  depName: string,
  opts: {
    storePath: string,
    prefix: string,
  },
): Promise<true | string> {
  try {
    const link = await isInnerLink(modules, depName)

    if (link.isInner) return true

    if (isSubdir(opts.storePath, link.target)) return true

    return link.target as string
  } catch (err) {
    if (err.code === 'ENOENT') return true

    logger.warn({
      message: `Moving ${depName} that was installed by a different package manager to "node_modules/.ignored`,
      prefix: opts.prefix,
    })
    const ignoredDir = path.join(modules, '.ignored', depName)
    await mkdirp(path.dirname(ignoredDir))
    await fs.rename(
      path.join(modules, depName),
      ignoredDir)
    return true
  }
}

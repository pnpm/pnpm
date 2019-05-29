import logger from '@pnpm/logger'
import isInnerLink = require('is-inner-link')
import isSubdir = require('is-subdir')
import makeDir = require('make-dir')
import fs = require('mz/fs')
import path = require('path')

export default async function safeIsInnerLink (
  importerModulesDir: string,
  depName: string,
  opts: {
    hideAlienModules: boolean,
    storePath: string,
    prefix: string,
  },
): Promise<true | string> {
  try {
    const link = await isInnerLink(importerModulesDir, depName)

    if (link.isInner) return true

    if (isSubdir(opts.storePath, link.target)) return true

    return link.target as string
  } catch (err) {
    if (err.code === 'ENOENT') return true

    if (opts.hideAlienModules) {
      logger.warn({
        message: `Moving ${depName} that was installed by a different package manager to "node_modules/.ignored`,
        prefix: opts.prefix,
      })
      const ignoredDir = path.join(importerModulesDir, '.ignored', depName)
      await makeDir(path.dirname(ignoredDir))
      await fs.rename(
        path.join(importerModulesDir, depName),
        ignoredDir,
      )
    }
    return true
  }
}

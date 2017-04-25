import logger from 'pnpm-logger'
import path = require('path')
import isInnerLink = require('is-inner-link')
import fs = require('mz/fs')
import mkdirp = require('mkdirp-promise')

export default async function safeIsInnerLink (modules: string, depName: string) {
  try {
    const link = await isInnerLink(modules, depName)

    if (link.isInner) return true

    logger.info(`${depName} is linked to ${modules} from ${link.target}`)
    return false
  } catch (err) {
    if (err.code === 'ENOENT') return true

    logger.warn(`Moving ${depName} that was installed by a different package manager to "node_modules/.ignored`)
    const ignoredDir = path.join(modules, '.ignored')
    await mkdirp(ignoredDir)
    await fs.rename(
      path.join(modules, depName),
      path.join(ignoredDir, depName))
    return true
  }
}

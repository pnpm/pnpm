import logger from 'pnpm-logger'
import path = require('path')
import isInnerLink = require('is-inner-link')
import rimraf = require('rimraf-then')

export default async function safeIsInnerLink (modules: string, depName: string) {
  try {
    const link = await isInnerLink(modules, depName)

    if (link.isInner) return true

    logger.info(`${depName} is linked to ${modules} from ${link.target}`)
    return false
  } catch (err) {
    if (err.code === 'ENOENT') return true

    logger.warn(`Removing ${depName} that was installed by a different package manager`)
    await rimraf(path.join(modules, depName))
    return true
  }
}

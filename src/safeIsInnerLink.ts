import logger from 'pnpm-logger'
import path = require('path')
import isInnerLink = require('is-inner-link')

export default async function safeIsInnerLink (modules: string, depName: string) {
  try {
    const link = await isInnerLink(modules, depName)

    if (link.isInner) return true

    logger.info(`${depName} is linked to ${modules} from ${link.target}`)
    return false
  } catch (err) {
    if (err.code === 'ENOENT') return true
    throw err
  }
}

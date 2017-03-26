import logger from 'pnpm-logger'
import path = require('path')
import getLinkTarget = require('get-link-target')

export default async function isInnerLink (modules: string, depName: string) {
  let linkTarget: string
  try {
    const linkPath = path.join(modules, depName)
    linkTarget = await getLinkTarget(linkPath)
  } catch (err) {
    if (err.code === 'ENOENT') return true
    throw err
  }

  if (linkTarget.startsWith(modules)) {
    return true
  }
  logger.info(`${depName} is linked to ${modules} from ${linkTarget}`)
  return false
}

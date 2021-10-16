import { promises as fs } from 'fs'
import path from 'path'
import logger from '@pnpm/logger'
import isInnerLink from 'is-inner-link'
import isSubdir from 'is-subdir'

export default async function safeIsInnerLink (
  projectModulesDir: string,
  depName: string,
  opts: {
    hideAlienModules: boolean
    projectDir: string
    storeDir: string
    virtualStoreDir: string
  }
): Promise<true | string> {
  try {
    const link = await isInnerLink(projectModulesDir, depName)

    if (link.isInner) return true

    if (isSubdir(opts.virtualStoreDir, link.target) || isSubdir(opts.storeDir, link.target)) return true

    return link.target as string
  } catch (err: any) { // eslint-disable-line
    if (err.code === 'ENOENT') return true

    if (opts.hideAlienModules) {
      logger.warn({
        message: `Moving ${depName} that was installed by a different package manager to "node_modules/.ignored`,
        prefix: opts.projectDir,
      })
      const ignoredDir = path.join(projectModulesDir, '.ignored', depName)
      await fs.mkdir(path.dirname(ignoredDir), { recursive: true })
      await fs.rename(
        path.join(projectModulesDir, depName),
        ignoredDir
      )
    }
    return true
  }
}

import path from 'path'
import { logger } from '@pnpm/logger'
import isInnerLink from 'is-inner-link'
import isSubdir from 'is-subdir'
import renameOverwrite from 'rename-overwrite'

export default async function safeIsInnerLink (
  projectModulesDir: string,
  depName: string,
  opts: {
    hideAlienModules: boolean
    projectDir: string
    virtualStoreDir: string
  }
): Promise<true | string> {
  try {
    const link = await isInnerLink(projectModulesDir, depName)

    if (link.isInner) return true

    if (isSubdir(opts.virtualStoreDir, link.target)) return true

    return link.target as string
  } catch (err: any) { // eslint-disable-line
    if (err.code === 'ENOENT') return true

    if (opts.hideAlienModules) {
      logger.warn({
        message: `Moving ${depName} that was installed by a different package manager to "node_modules/.ignored`,
        prefix: opts.projectDir,
      })
      const ignoredDir = path.join(projectModulesDir, '.ignored', depName)
      await renameOverwrite(
        path.join(projectModulesDir, depName),
        ignoredDir
      )
    }
    return true
  }
}

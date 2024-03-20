import path from 'node:path'

import isSubdir from 'is-subdir'
import { logger } from '@pnpm/logger'
import isInnerLink from 'is-inner-link'
import renameOverwrite from 'rename-overwrite'

export async function safeIsInnerLink(
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

    if (link.isInner) {
      return true
    }

    if (isSubdir(opts.virtualStoreDir, link.target)) {
      return true
    }

    return link.target as string
  } catch (err: unknown) {
    // @ts-ignore
    if (err.code === 'ENOENT') {
      return true
    }

    if (opts.hideAlienModules) {
      logger.warn({
        message: `Moving ${depName} that was installed by a different package manager to "node_modules/.ignored"`,
        prefix: opts.projectDir,
      })

      const ignoredDir = path.join(projectModulesDir, '.ignored', depName)

      await renameOverwrite(path.join(projectModulesDir, depName), ignoredDir)
    }

    return true
  }
}

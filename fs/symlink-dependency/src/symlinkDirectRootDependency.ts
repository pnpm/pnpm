import path from 'node:path'
import { promises as fs } from 'node:fs'

import symlinkDir from 'symlink-dir'

import { rootLogger } from '@pnpm/core-loggers'
import type { DependenciesField, DependencyType } from '@pnpm/types'

const DEP_TYPE_BY_DEPS_FIELD_NAME = {
  dependencies: 'prod',
  devDependencies: 'dev',
  optionalDependencies: 'optional',
} as const

export async function symlinkDirectRootDependency(
  dependencyLocation: string,
  destModulesDir: string,
  importAs: string,
  opts: {
    fromDependenciesField?: DependenciesField | undefined
    linkedPackage: {
      name: string
      version: string
    }
    prefix: string
  }
): Promise<void> {
  // `opts.destModulesDir` may be a non-existent `node_modules` dir
  // so `fs.realpath` would throw.
  // Even though `symlinkDir` creates the dir if it doesn't exist,
  // our dir may include an ancestor dir which is symlinked,
  // so we create it if it doesn't exist, and then find its realpath.
  let destModulesDirReal

  try {
    destModulesDirReal = await fs.realpath(destModulesDir)
  } catch (err: unknown) {
    // @ts-ignore
    if (err.code === 'ENOENT') {
      await fs.mkdir(destModulesDir, { recursive: true })

      destModulesDirReal = await fs.realpath(destModulesDir)
    } else {
      throw err
    }
  }

  const dest = path.join(destModulesDirReal, importAs)

  const { reused } = await symlinkDir(dependencyLocation, dest)

  // if the link was already present, don't log
  if (reused) {
    return
  }

  rootLogger.debug({
    added: {
      dependencyType:
        opts.fromDependenciesField &&
        (DEP_TYPE_BY_DEPS_FIELD_NAME[
          opts.fromDependenciesField
        ]),
      linkedFrom: dependencyLocation,
      name: importAs,
      realName: opts.linkedPackage.name,
      version: opts.linkedPackage.version,
    },
    prefix: opts.prefix,
  })
}

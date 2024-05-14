import { promises as fs } from 'fs'
import path from 'path'
import util from 'util'
import {
  type DependencyType,
  rootLogger,
} from '@pnpm/core-loggers'
import { type DependenciesField } from '@pnpm/types'
import symlinkDir from 'symlink-dir'

const DEP_TYPE_BY_DEPS_FIELD_NAME = {
  dependencies: 'prod',
  devDependencies: 'dev',
  optionalDependencies: 'optional',
}

export async function symlinkDirectRootDependency (
  dependencyLocation: string,
  destModulesDir: string,
  importAs: string,
  opts: {
    fromDependenciesField?: DependenciesField
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
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      await fs.mkdir(destModulesDir, { recursive: true })
      destModulesDirReal = await fs.realpath(destModulesDir)
    } else {
      throw err
    }
  }

  const dest = path.join(destModulesDirReal, importAs)
  const { reused } = await symlinkDir(dependencyLocation, dest)
  if (reused) return // if the link was already present, don't log
  rootLogger.debug({
    added: {
      dependencyType: opts.fromDependenciesField && DEP_TYPE_BY_DEPS_FIELD_NAME[opts.fromDependenciesField] as DependencyType,
      linkedFrom: dependencyLocation,
      name: importAs,
      realName: opts.linkedPackage.name,
      version: opts.linkedPackage.version,
    },
    prefix: opts.prefix,
  })
}

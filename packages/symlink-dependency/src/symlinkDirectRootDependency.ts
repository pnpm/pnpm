import {
  DependencyType,
  rootLogger,
} from '@pnpm/core-loggers'
import { DependenciesField } from '@pnpm/types'
import makeDir = require('make-dir')
import fs = require('mz/fs')
import path = require('path')
import symlinkDir = require('symlink-dir')

const DEP_TYPE_BY_DEPS_FIELD_NAME = {
  dependencies: 'prod',
  devDependencies: 'dev',
  optionalDependencies: 'optional',
}

export default async function symlinkDirectRootDependency (
  dependencyLocation: string,
  destModulesDir: string,
  importAs: string,
  opts: {
    fromDependenciesField?: DependenciesField,
    linkedPackage: {
      name: string,
      version: string,
    },
    prefix: string,
  },
) {
  // `opts.destModulesDir` may be a non-existent `node_modules` dir
  // so `fs.realpath` would throw.
  // Even though `symlinkDir` creates the dir if it doesn't exist,
  // our dir may include an ancestor dir which is symlinked,
  // so we create it if it doesn't exist, and then find its realpath.
  let destModulesDirReal
  try {
    destModulesDirReal = await fs.realpath(destModulesDir)
  } catch (err) {
    if (err.code === 'ENOENT') {
      await makeDir(destModulesDir)
      destModulesDirReal = await fs.realpath(destModulesDir)
    } else {
      throw err
    }
  }

  const dependencyRealocation = await fs.realpath(dependencyLocation)

  const dest = path.join(destModulesDirReal, importAs)
  const { reused } = await symlinkDir(dependencyRealocation, dest)
  if (reused) return // if the link was already present, don't log
  rootLogger.debug({
    added: {
      dependencyType: opts.fromDependenciesField && DEP_TYPE_BY_DEPS_FIELD_NAME[opts.fromDependenciesField] as DependencyType,
      linkedFrom: dependencyRealocation,
      name: importAs,
      realName: opts.linkedPackage.name,
      version: opts.linkedPackage.version,
    },
    prefix: opts.prefix,
  })
}

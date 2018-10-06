import {
  DependencyType,
  rootLogger,
} from '@pnpm/core-loggers'
import { DependenciesField } from '@pnpm/types'
import mkdirp = require('mkdirp-promise')
import fs = require('mz/fs')
import path = require('path')
import symlinkDir = require('symlink-dir')

const DEP_TYPE_BY_DEPS_FIELD_NAME = {
  dependencies: 'prod',
  devDependencies: 'dev',
  optionalDependencies: 'optional',
}

export default async function linkToModules (
  opts: {
    alias: string,
    destModulesDir: string,
    name: string,
    packageDir: string,
    prefix: string,
    saveType?: DependenciesField,
    version: string,
  },
) {
  // `opts.destModulesDir` may be a non-existent `node_modules` dir
  // so `fs.realpath` would throw.
  // Even though `symlinkDir` creates the dir if it doesn't exist,
  // our dir may include an ancestor dir which is symlinked,
  // so we create it if it doesn't exist, and then find its realpath.
  let destModulesDirReal
  try {
    destModulesDirReal = await fs.realpath(opts.destModulesDir)
  } catch (err) {
    if (err.code === 'ENOENT') {
      await mkdirp(opts.destModulesDir)
      destModulesDirReal = await fs.realpath(opts.destModulesDir)
    } else {
      throw err
    }
  }

  const packageDirReal = await fs.realpath(opts.packageDir)

  const dest = path.join(destModulesDirReal, opts.alias)
  const { reused } = await symlinkDir(packageDirReal, dest)
  if (reused) return // if the link was already present, don't log
  rootLogger.debug({
    added: {
      dependencyType: opts.saveType && DEP_TYPE_BY_DEPS_FIELD_NAME[opts.saveType] as DependencyType,
      linkedFrom: packageDirReal,
      name: opts.alias,
      realName: opts.name,
      version: opts.version,
    },
    prefix: opts.prefix,
  })
}

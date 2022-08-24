import { promises as fs } from 'fs'
import path from 'path'
import {
  DependencyType,
  rootLogger,
} from '@pnpm/core-loggers'
import { globalWarn } from '@pnpm/logger'
import { DependenciesField } from '@pnpm/types'
import symlinkDir from 'symlink-dir'

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
    fromDependenciesField?: DependenciesField
    linkedPackage: {
      name: string
      version: string
    }
    prefix: string
  }
) {
  // `opts.destModulesDir` may be a non-existent `node_modules` dir
  // so `fs.realpath` would throw.
  // Even though `symlinkDir` creates the dir if it doesn't exist,
  // our dir may include an ancestor dir which is symlinked,
  // so we create it if it doesn't exist, and then find its realpath.
  let destModulesDirReal
  try {
    destModulesDirReal = await fs.realpath(destModulesDir)
  } catch (err: any) { // eslint-disable-line
    if (err.code === 'ENOENT') {
      await fs.mkdir(destModulesDir, { recursive: true })
      destModulesDirReal = await fs.realpath(destModulesDir)
    } else {
      throw err
    }
  }

  let dependencyRealLocation!: string
  try {
    dependencyRealLocation = await fs.realpath(dependencyLocation)
  } catch (err: any) { // eslint-disable-line
    if (err.code !== 'ENOENT') throw err
    globalWarn(`Local dependency not found at ${dependencyLocation}`)
    // Sometimes the linked in local package does not exist during installation
    // and is created later via a build script.
    // That is why we create the symlink even if the target directory does not exist.
    dependencyRealLocation = dependencyLocation
  }

  const dest = path.join(destModulesDirReal, importAs)
  const { reused } = await symlinkDir(dependencyRealLocation, dest)
  if (reused) return // if the link was already present, don't log
  rootLogger.debug({
    added: {
      dependencyType: opts.fromDependenciesField && DEP_TYPE_BY_DEPS_FIELD_NAME[opts.fromDependenciesField] as DependencyType,
      linkedFrom: dependencyRealLocation,
      name: importAs,
      realName: opts.linkedPackage.name,
      version: opts.linkedPackage.version,
    },
    prefix: opts.prefix,
  })
}

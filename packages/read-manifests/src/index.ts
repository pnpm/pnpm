import { read as readModulesYaml } from '@pnpm/modules-yaml'
import { PackageJson } from '@pnpm/types'
import {
  DEFAULT_REGISTRIES,
  normalizeRegistries,
  realNodeModulesDir,
  safeReadPackageFromDir as safeReadPkgFromDir,
} from '@pnpm/utils'
import path = require('path')
import { getImporterId, Shrinkwrap } from 'pnpm-shrinkwrap'

export interface ImporterOptions {
  bin?: string,
  prefix: string,
  shamefullyFlatten?: boolean,
}

export default async (
  importers: ImporterOptions[],
  shrinkwrapDirectory: string,
  opts: {
    shamefullyFlatten: boolean,
  },
) => {
  const virtualStoreDir = await realNodeModulesDir(shrinkwrapDirectory)
  const modules = await readModulesYaml(virtualStoreDir)
  return {
    importers: await Promise.all(
      importers.map(async (importer) => {
        let pkg = await safeReadPkgFromDir(importer.prefix) || {} as PackageJson
        const modulesDir = await realNodeModulesDir(importer.prefix)
        const importerId = getImporterId(shrinkwrapDirectory, importer.prefix)

        return {
          bin: importer.bin || path.join(importer.prefix, 'node_modules', '.bin'),
          hoistedAliases: modules && modules.importers[importerId] && modules.importers[importerId].hoistedAliases || {},
          id: importerId,
          modulesDir,
          pkg,
          prefix: importer.prefix,
          currentShamefullyFlatten: modules && modules.importers[importerId] && modules.importers[importerId].shamefullyFlatten,
          shamefullyFlatten: Boolean(
            typeof importer.shamefullyFlatten === 'boolean' ? importer.shamefullyFlatten : opts.shamefullyFlatten
          ),
        }
      })),
    modules,
    include: modules && modules.included || { dependencies: true, devDependencies: true, optionalDependencies: true },
    pendingBuilds: modules && modules.pendingBuilds || [],
    registries: modules && modules.registries && normalizeRegistries(modules.registries),
    skipped: new Set(modules && modules.skipped || []),
    virtualStoreDir,
  }
}

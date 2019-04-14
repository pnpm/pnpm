import { getLockfileImporterId } from '@pnpm/lockfile-file'
import { Modules, read as readModulesYaml } from '@pnpm/modules-yaml'
import { Registries } from '@pnpm/types'
import {
  normalizeRegistries,
  realNodeModulesDir,
} from '@pnpm/utils'
import path = require('path')

export interface ImporterOptions {
  bin?: string,
  prefix: string,
  shamefullyFlatten?: boolean,
}

export default async (
  importers: ImporterOptions[],
  lockfileDirectory: string,
  opts: {
    shamefullyFlatten: boolean,
  },
): Promise<{
  importers: Array<{
    bin: string,
    currentShamefullyFlatten: boolean | null,
    hoistedAliases: { [depPath: string]: string[] },
    id: string,
    modulesDir: string,
    prefix: string,
    shamefullyFlatten: boolean,
  }>,
  include: {
    dependencies: boolean,
    devDependencies: boolean,
    optionalDependencies: boolean,
  },
  modules: Modules | null,
  pendingBuilds: string[],
  registries: Registries | null | undefined,
  skipped: Set<string>,
  virtualStoreDir: string,
}> => {
  const virtualStoreDir = await realNodeModulesDir(lockfileDirectory)
  const modules = await readModulesYaml(virtualStoreDir)
  return {
    importers: await Promise.all(
      importers.map(async (importer) => {
        const modulesDir = await realNodeModulesDir(importer.prefix)
        const importerId = getLockfileImporterId(lockfileDirectory, importer.prefix)

        return {
          bin: importer.bin || path.join(importer.prefix, 'node_modules', '.bin'),
          currentShamefullyFlatten: modules && modules.importers[importerId] && modules.importers[importerId].shamefullyFlatten,
          hoistedAliases: modules && modules.importers[importerId] && modules.importers[importerId].hoistedAliases || {},
          id: importerId,
          modulesDir,
          prefix: importer.prefix,
          shamefullyFlatten: Boolean(
            typeof importer.shamefullyFlatten === 'boolean' ? importer.shamefullyFlatten : opts.shamefullyFlatten
          ),
        }
      })),
    include: modules && modules.included || { dependencies: true, devDependencies: true, optionalDependencies: true },
    modules,
    pendingBuilds: modules && modules.pendingBuilds || [],
    registries: modules && modules.registries && normalizeRegistries(modules.registries),
    skipped: new Set(modules && modules.skipped || []),
    virtualStoreDir,
  }
}

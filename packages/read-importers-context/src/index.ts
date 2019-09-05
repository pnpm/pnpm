import { getLockfileImporterId } from '@pnpm/lockfile-file'
import { Modules, read as readModulesYaml } from '@pnpm/modules-yaml'
import { DependenciesField, Registries } from '@pnpm/types'
import {
  normalizeRegistries,
  realNodeModulesDir,
} from '@pnpm/utils'
import path = require('path')

export interface ImporterOptions {
  bin?: string,
  prefix: string,
}

export default async function <T>(
  importers: (ImporterOptions & T)[],
  lockfileDirectory: string,
): Promise<{
  currentHoistPattern?: string,
  hoistedAliases: { [depPath: string]: string[] },
  importers: Array<{
    id: string,
    modulesDir: string,
  } & T & Required<ImporterOptions>>,
  include: Record<DependenciesField, boolean>,
  modules: Modules | null,
  pendingBuilds: string[],
  registries: Registries | null | undefined,
  skipped: Set<string>,
  virtualStoreDir: string,
}> {
  const virtualStoreDir = await realNodeModulesDir(lockfileDirectory)
  const modules = await readModulesYaml(virtualStoreDir)
  return {
    currentHoistPattern: modules && modules.hoistPattern || undefined,
    hoistedAliases: modules && modules.hoistedAliases || {},
    importers: await Promise.all(
      importers.map(async (importer) => {
        const modulesDir = await realNodeModulesDir(importer.prefix)
        const importerId = getLockfileImporterId(lockfileDirectory, importer.prefix)

        return {
          ...importer,
          bin: importer.bin || path.join(importer.prefix, 'node_modules', '.bin'),
          id: importerId,
          modulesDir,
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

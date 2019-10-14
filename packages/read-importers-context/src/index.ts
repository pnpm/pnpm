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
  currentHoistPattern?: string[],
  hoist?: boolean,
  hoistedAliases: { [depPath: string]: string[] },
  importers: Array<{
    id: string,
    modulesDir: string,
  } & T & Required<ImporterOptions>>,
  include: Record<DependenciesField, boolean>,
  independentLeaves: boolean | undefined,
  modules: Modules | null,
  pendingBuilds: string[],
  registries: Registries | null | undefined,
  rootModulesDir: string,
  shamefullyHoist?: boolean,
  skipped: Set<string>,
}> {
  const rootModulesDir = await realNodeModulesDir(lockfileDirectory)
  const modules = await readModulesYaml(rootModulesDir)
  return {
    currentHoistPattern: modules?.hoistPattern || undefined,
    hoist: !modules ? undefined : Boolean(modules.hoistPattern),
    hoistedAliases: modules?.hoistedAliases || {},
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
    include: modules?.included || { dependencies: true, devDependencies: true, optionalDependencies: true },
    independentLeaves: modules?.independentLeaves || undefined,
    modules,
    pendingBuilds: modules?.pendingBuilds || [],
    registries: modules?.registries && normalizeRegistries(modules.registries),
    rootModulesDir,
    shamefullyHoist: modules?.shamefullyHoist || undefined,
    skipped: new Set(modules?.skipped || []),
  }
}

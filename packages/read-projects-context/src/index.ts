import { getLockfileImporterId } from '@pnpm/lockfile-file'
import { Modules, read as readModulesYaml } from '@pnpm/modules-yaml'
import normalizeRegistries from '@pnpm/normalize-registries'
import { DependenciesField, Registries } from '@pnpm/types'
import {
  realNodeModulesDir,
} from '@pnpm/utils'
import path = require('path')

export interface ProjectOptions {
  binsDir?: string,
  rootDir: string,
}

export default async function <T>(
  projects: (ProjectOptions & T)[],
  lockfileDir: string,
): Promise<{
  currentHoistPattern?: string[],
  hoist?: boolean,
  hoistedAliases: { [depPath: string]: string[] },
  projects: Array<{
    id: string,
    modulesDir: string,
  } & T & Required<ProjectOptions>>,
  include: Record<DependenciesField, boolean>,
  independentLeaves: boolean | undefined,
  modules: Modules | null,
  pendingBuilds: string[],
  registries: Registries | null | undefined,
  rootModulesDir: string,
  shamefullyHoist?: boolean,
  skipped: Set<string>,
}> {
  const rootModulesDir = await realNodeModulesDir(lockfileDir)
  const modules = await readModulesYaml(rootModulesDir)
  return {
    currentHoistPattern: modules?.hoistPattern || undefined,
    hoist: !modules ? undefined : Boolean(modules.hoistPattern),
    hoistedAliases: modules?.hoistedAliases || {},
    include: modules?.included || { dependencies: true, devDependencies: true, optionalDependencies: true },
    independentLeaves: modules?.independentLeaves || undefined,
    modules,
    pendingBuilds: modules?.pendingBuilds || [],
    projects: await Promise.all(
      projects.map(async (project) => {
        const modulesDir = await realNodeModulesDir(project.rootDir)
        const importerId = getLockfileImporterId(lockfileDir, project.rootDir)

        return {
          ...project,
          binsDir: project.binsDir || path.join(project.rootDir, 'node_modules', '.bin'),
          id: importerId,
          modulesDir,
        }
      })),
    registries: modules?.registries && normalizeRegistries(modules.registries),
    rootModulesDir,
    shamefullyHoist: modules?.shamefullyHoist || undefined,
    skipped: new Set(modules?.skipped || []),
  }
}

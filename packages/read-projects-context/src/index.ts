import { getLockfileImporterId } from '@pnpm/lockfile-file'
import { Modules, read as readModulesYaml } from '@pnpm/modules-yaml'
import normalizeRegistries from '@pnpm/normalize-registries'
import { DependenciesField, Registries } from '@pnpm/types'
import path = require('path')
import realpathMissing = require('realpath-missing')

export interface ProjectOptions {
  binsDir?: string,
  modulesDir?: string,
  rootDir: string,
}

export default async function <T>(
  projects: (ProjectOptions & T)[],
  opts: {
    lockfileDir: string,
    modulesDir?: string,
  },
): Promise<{
  currentHoistPattern?: string[],
  hoist?: boolean,
  hoistedAliases: { [depPath: string]: string[] },
  projects: Array<{
    id: string,
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
  const relativeModulesDir = opts.modulesDir ?? 'node_modules'
  const rootModulesDir = await realpathMissing(path.join(opts.lockfileDir, relativeModulesDir))
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
        const modulesDir = await realpathMissing(path.join(project.rootDir, project.modulesDir ?? relativeModulesDir))
        const importerId = getLockfileImporterId(opts.lockfileDir, project.rootDir)

        return {
          ...project,
          binsDir: project.binsDir ?? path.join(project.rootDir, relativeModulesDir, '.bin'),
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

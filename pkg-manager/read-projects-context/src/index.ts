import path from 'path'
import { getLockfileImporterId } from '@pnpm/lockfile-file'
import { type Modules, readModulesManifest } from '@pnpm/modules-yaml'
import { normalizeRegistries } from '@pnpm/normalize-registries'
import { type DependenciesField, type HoistedDependencies, type Registries } from '@pnpm/types'
import realpathMissing from 'realpath-missing'

export interface ProjectOptions {
  binsDir?: string
  modulesDir?: string
  rootDir: string
}

export async function readProjectsContext<T> (
  projects: Array<ProjectOptions & T>,
  opts: {
    lockfileDir: string
    modulesDir?: string
  }
): Promise<{
    currentHoistPattern?: string[]
    currentPublicHoistPattern?: string[]
    hoist?: boolean
    hoistedDependencies: HoistedDependencies
    projects: Array<{
      id: string
    } & T & Required<ProjectOptions>>
    include: Record<DependenciesField, boolean>
    modules: Modules | null
    pendingBuilds: string[]
    registries: Registries | null | undefined
    rootModulesDir: string
    skipped: Set<string>
    virtualStoreDirMaxLength?: number
  }> {
  const relativeModulesDir = opts.modulesDir ?? 'node_modules'
  const rootModulesDir = await realpathMissing(path.join(opts.lockfileDir, relativeModulesDir))
  const modules = await readModulesManifest(rootModulesDir)
  return {
    currentHoistPattern: modules?.hoistPattern,
    currentPublicHoistPattern: modules?.publicHoistPattern,
    hoist: (modules == null) ? undefined : Boolean(modules.hoistPattern),
    hoistedDependencies: modules?.hoistedDependencies ?? {},
    include: modules?.included ?? { dependencies: true, devDependencies: true, optionalDependencies: true },
    modules,
    pendingBuilds: modules?.pendingBuilds ?? [],
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
    registries: ((modules?.registries) != null) ? normalizeRegistries(modules.registries) : undefined,
    rootModulesDir,
    skipped: new Set(modules?.skipped ?? []),
    virtualStoreDirMaxLength: modules?.virtualStoreDirMaxLength,
  }
}

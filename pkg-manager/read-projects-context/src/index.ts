import '@total-typescript/ts-reset'

import path from 'node:path'

import realpathMissing from 'realpath-missing'

import type {
  Modules,
  Registries,
  ProjectOptions,
  DependenciesField,
  HoistedDependencies,
} from '@pnpm/types'
import { readModulesManifest } from '@pnpm/modules-yaml'
import { getLockfileImporterId } from '@pnpm/lockfile-file'
import { normalizeRegistries } from '@pnpm/normalize-registries'

export async function readProjectsContext<T>(
  projects: Array<ProjectOptions & T> | undefined,
  opts: {
    lockfileDir: string
    modulesDir?: string | undefined
  }
): Promise<{
    currentHoistPattern?: string[] | undefined
    currentPublicHoistPattern?: string[] | undefined
    hoist?: boolean | undefined
    hoistedDependencies: HoistedDependencies
    projects: Awaited<ProjectOptions & T & { binsDir: string; id: string; modulesDir: string; }>[]
    include: Record<DependenciesField, boolean>
    modules: Modules | null
    pendingBuilds: string[]
    registries: Registries | null | undefined
    rootModulesDir: string
    skipped: Set<string>
  }> {
  const relativeModulesDir = opts.modulesDir ?? 'node_modules'

  const rootModulesDir = await realpathMissing(
    path.join(opts.lockfileDir, relativeModulesDir)
  )

  const modules = await readModulesManifest(rootModulesDir)

  return {
    currentHoistPattern: modules?.hoistPattern,
    currentPublicHoistPattern: modules?.publicHoistPattern,
    hoist: modules == null ? undefined : Boolean(modules.hoistPattern),
    hoistedDependencies: modules?.hoistedDependencies ?? {},
    include: modules?.included ?? {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    modules,
    pendingBuilds: modules?.pendingBuilds ?? [],
    projects: await Promise.all(
      (projects ?? []).map(async (project: ProjectOptions & T): Promise<ProjectOptions & T & {
        binsDir: string;
        id: string;
        modulesDir: string;
      }> => {
        const modulesDir = await realpathMissing(
          path.join(project.rootDir, project.modulesDir ?? relativeModulesDir)
        )

        const importerId = getLockfileImporterId(
          opts.lockfileDir,
          project.rootDir
        )

        return {
          ...project,
          binsDir:
            project.binsDir ??
            path.join(project.rootDir, relativeModulesDir, '.bin'),
          id: importerId,
          modulesDir,
        }
      })
    ),
    registries:
      modules?.registries != null
        ? normalizeRegistries(modules.registries)
        : undefined,
    rootModulesDir,
    skipped: new Set(modules?.skipped ?? []),
  }
}

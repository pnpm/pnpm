import { promises as fs } from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { normalizeRegistries } from '@pnpm/config.normalize-registries'
import { type Modules, readModulesManifest } from '@pnpm/installing.modules-yaml'
import { getLockfileImporterId } from '@pnpm/lockfile.fs'
import type {
  DependenciesField,
  DepPath,
  HoistedDependencies,
  ProjectId,
  ProjectRootDir,
  ProjectRootDirRealPath,
  Registries,
} from '@pnpm/types'
import { pathAbsolute } from 'path-absolute'
import { realpathMissing } from 'realpath-missing'

export interface ProjectOptions {
  binsDir?: string
  modulesDir?: string
  rootDir: ProjectRootDir
  rootDirRealPath?: ProjectRootDirRealPath
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
    id: ProjectId
  } & T & Required<ProjectOptions>>
  include: Record<DependenciesField, boolean>
  modules: Modules | null
  pendingBuilds: string[]
  registries: Registries | null | undefined
  rootModulesDir: string
  skipped: Set<DepPath>
  virtualStoreDirMaxLength?: number
}> {
  // `modulesDir` is conventionally a path relative to `lockfileDir`, but
  // some callers pass it as an absolute path. Resolve via `pathAbsolute`
  // so both forms work — `path.join` on Windows would otherwise produce a
  // doubled prefix when the second argument is also absolute.
  const modulesDirOpt = opts.modulesDir ?? 'node_modules'
  const rootModulesDir = await realpathMissing(pathAbsolute(modulesDirOpt, opts.lockfileDir))
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
        const modulesDir = await realpathMissing(pathAbsolute(project.modulesDir ?? modulesDirOpt, project.rootDir))
        const importerId = getLockfileImporterId(opts.lockfileDir, project.rootDir)

        return {
          ...project,
          binsDir: project.binsDir ?? path.join(modulesDir, '.bin'),
          id: importerId,
          modulesDir,
          rootDirRealPath: project.rootDirRealPath ?? await realpath(project.rootDir),
        }
      })),
    registries: ((modules?.registries) != null) ? normalizeRegistries(modules.registries) : undefined,
    rootModulesDir,
    skipped: new Set((modules?.skipped ?? []) as DepPath[]),
    virtualStoreDirMaxLength: modules?.virtualStoreDirMaxLength,
  }
}

async function realpath (path: ProjectRootDir): Promise<ProjectRootDirRealPath> {
  try {
    return await fs.realpath(path) as ProjectRootDirRealPath
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return path as unknown as ProjectRootDirRealPath
    }
    throw err
  }
}

import fs from 'node:fs'
import path from 'node:path'

import { type Config, type ConfigContext, createProjectModulesDirResolver } from '@pnpm/config.reader'
import { DEPENDENCIES_FIELDS, type IncludedDependencies, type ProjectManifest } from '@pnpm/types'

export type FindDanglingDirectDependencyLinkOptions = Pick<Config, 'lockfileDir' | 'modulesDir' | 'packageConfigs'>
& Pick<ConfigContext, 'allProjects' | 'rootProjectManifest' | 'rootProjectManifestDir'>
& {
  include?: IncludedDependencies
}

/**
 * Returns the path of the first direct dependency whose entry in its
 * project's modules directory is a link to a missing target. Nothing the
 * up-to-date check reads moves when such a link breaks, while the full
 * install relinks it
 * (https://github.com/pnpm/pnpm/issues/9758). A missing entry is not a
 * broken link: skipped optional and excluded dependencies have none, and a
 * healthy entry costs one `stat`.
 */
export async function findDanglingDirectDependencyLink (opts: FindDanglingDirectDependencyLinkOptions): Promise<string | undefined> {
  const projects: Array<{ rootDir: string, manifest: ProjectManifest }> = opts.allProjects ??
    (opts.rootProjectManifest == null ? [] : [{ rootDir: opts.rootProjectManifestDir, manifest: opts.rootProjectManifest }])
  const modulesDirOf = createProjectModulesDirResolver(opts)
  const fields = DEPENDENCIES_FIELDS.filter((field) => opts.include?.[field] !== false)
  const entries = projects.flatMap(({ rootDir, manifest }) => {
    const modulesDir = path.resolve(rootDir, modulesDirOf(manifest.name) ?? 'node_modules')
    return fields.flatMap((field) => Object.keys(manifest[field] ?? {}).map((alias) => path.join(modulesDir, alias)))
  })
  const dangling = await Promise.all(entries.map(isDanglingLink))
  return entries.find((_, index) => dangling[index])
}

async function isDanglingLink (entry: string): Promise<boolean> {
  try {
    await fs.promises.stat(entry)
    return false
  } catch {
    try {
      await fs.promises.lstat(entry)
      return true
    } catch {
      return false
    }
  }
}

import fs from 'node:fs'
import path from 'node:path'

import { type Config, type ConfigContext, createProjectModulesDirResolver } from '@pnpm/config.reader'
import { isError } from '@pnpm/error'
import { DEPENDENCIES_FIELDS, type IncludedDependencies, type ProjectManifest } from '@pnpm/types'

export type FindDanglingDirectDependencyLinkOptions = Pick<Config, 'lockfileDir' | 'modulesDir' | 'nodeLinker' | 'packageConfigs'>
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
 * healthy entry costs one `stat`. The hoisted linker places a project's
 * dependencies in the root modules directory too, so both are probed there.
 */
export async function findDanglingDirectDependencyLink (opts: FindDanglingDirectDependencyLinkOptions): Promise<string | undefined> {
  const entries = listDirectDependencyEntries(opts)
  const dangling = await Promise.all(entries.map(isDanglingLink))
  return entries.find((_, index) => dangling[index])
}

function listDirectDependencyEntries (opts: FindDanglingDirectDependencyLinkOptions): string[] {
  const projects: Array<{ rootDir: string, manifest: ProjectManifest }> = [...(opts.allProjects ?? [])]
  if (opts.rootProjectManifest != null && !projects.some(({ rootDir }) => rootDir === opts.rootProjectManifestDir)) {
    projects.push({ rootDir: opts.rootProjectManifestDir, manifest: opts.rootProjectManifest })
  }
  const modulesDirOf = createProjectModulesDirResolver(opts)
  const rootModulesDir = path.resolve(opts.rootProjectManifestDir, modulesDirOf(opts.rootProjectManifest?.name) ?? 'node_modules')
  const fields = DEPENDENCIES_FIELDS.filter((field) => opts.include?.[field] !== false)
  return projects.flatMap(({ rootDir, manifest }) => {
    const modulesDirs = [path.resolve(rootDir, modulesDirOf(manifest.name) ?? 'node_modules')]
    if (opts.nodeLinker === 'hoisted' && modulesDirs[0] !== rootModulesDir) {
      modulesDirs.push(rootModulesDir)
    }
    const aliases = fields.flatMap((field) => Object.keys(manifest[field] ?? {}))
    return aliases.flatMap((alias) => modulesDirs.map((modulesDir) => path.join(modulesDir, alias)))
  })
}

async function isDanglingLink (entry: string): Promise<boolean> {
  try {
    await fs.promises.stat(entry)
    return false
  } catch (error) {
    if (!hasErrorCode(error, 'ENOENT', 'ENOTDIR', 'ELOOP')) throw error
  }
  try {
    await fs.promises.lstat(entry)
    return true
  } catch (error) {
    if (hasErrorCode(error, 'ENOENT', 'ENOTDIR')) return false
    throw error
  }
}

function hasErrorCode (error: unknown, ...codes: string[]): boolean {
  return isError(error) && 'code' in error && codes.includes(String(error.code))
}

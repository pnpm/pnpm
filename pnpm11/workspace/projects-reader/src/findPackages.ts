import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { lexCompare } from '@pnpm/text.ordinal-comparator'
import type { Project, ProjectRootDir, ProjectRootDirRealPath } from '@pnpm/types'
import { createManifestExclusionMatcher, normalizePatterns } from '@pnpm/workspace.package-patterns'
import { readExactProjectManifest, readExactProjectManifestSync } from '@pnpm/workspace.project-manifest-reader'
import pFilter from 'p-filter'
import { glob, globSync } from 'tinyglobby'

const DEFAULT_IGNORE = [
  '**/node_modules/**',
  '**/bower_components/**',
  '**/test/**',
  '**/tests/**',
]

export interface FindPackagesOptions {
  ignore?: string[]
  includeRoot?: boolean
  patterns?: string[]
}

export async function findPackages (root: string, opts?: FindPackagesOptions): Promise<Project[]> {
  opts = opts ?? {}
  const globOpts = { ...opts, cwd: root, expandDirectories: false }
  globOpts.ignore = opts.ignore ?? DEFAULT_IGNORE
  const patterns = opts.patterns ?? ['.', '**']
  delete globOpts.patterns
  const paths: string[] = excludeManifests(await glob(normalizePatterns(patterns), globOpts), patterns)

  if (opts.includeRoot) {
    // Always include the workspace root (https://github.com/pnpm/pnpm/issues/1986)
    paths.push(...(await glob(normalizePatterns(['.']), globOpts)))
  }

  return pFilter(
    Array.from(
      pickManifestPerDirectory(root, paths),
      async manifestPath => {
        try {
          const rootDir = path.dirname(manifestPath) as ProjectRootDir
          return {
            rootDir,
            rootDirRealPath: await fs.promises.realpath(rootDir) as ProjectRootDirRealPath,
            ...await readExactProjectManifest(manifestPath),
          } as Project
        } catch (err: unknown) {
          if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
            return null!
          }
          throw err
        }
      }),
    Boolean
  )
}

export function findPackagesSync (root: string, opts?: FindPackagesOptions): Project[] {
  opts = opts ?? {}
  const globOpts = { ...opts, cwd: root, expandDirectories: false }
  globOpts.ignore = opts.ignore ?? DEFAULT_IGNORE
  const patterns = opts.patterns ?? ['.', '**']
  delete globOpts.patterns
  const paths: string[] = excludeManifests(globSync(normalizePatterns(patterns), globOpts), patterns)

  if (opts.includeRoot) {
    paths.push(...globSync(normalizePatterns(['.']), globOpts))
  }

  const uniquePaths = pickManifestPerDirectory(root, paths)

  const projects: Project[] = []
  for (const manifestPath of uniquePaths) {
    try {
      const rootDir = path.dirname(manifestPath) as ProjectRootDir
      projects.push({
        rootDir,
        rootDirRealPath: fs.realpathSync(rootDir) as ProjectRootDirRealPath,
        ...readExactProjectManifestSync(manifestPath),
      } as Project)
    } catch (err: unknown) {
      if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
        continue
      }
      throw err
    }
  }
  return projects
}

/**
 * tinyglobby applies the negated patterns without letting their wildcards
 * match dot directories, so the exclusions are applied again with the
 * semantics the membership check uses.
 */
function excludeManifests (manifestPaths: string[], patterns: readonly string[]): string[] {
  const isExcluded = createManifestExclusionMatcher(patterns)
  return manifestPaths.filter((manifestPath) => !isExcluded(manifestPath))
}

function pickManifestPerDirectory (root: string, paths: string[]): string[] {
  const byDir = new Map<string, string>()
  for (const manifestPath of paths) {
    const fullPath = path.join(root, manifestPath)
    const dir = path.dirname(fullPath)
    const selected = byDir.get(dir)
    // The supported names sort in reader order: package.json, package.json5, package.yaml.
    if (selected == null || lexCompare(path.basename(fullPath), path.basename(selected)) < 0) {
      byDir.set(dir, fullPath)
    }
  }
  return Array.from(byDir.values()).sort((path1, path2) =>
    lexCompare(path.dirname(path1), path.dirname(path2))
  )
}

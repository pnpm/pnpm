import { promises as fs } from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import type { Project, ProjectRootDir, ProjectRootDirRealPath } from '@pnpm/types'
import { lexCompare } from '@pnpm/util.lex-comparator'
import { readExactProjectManifest } from '@pnpm/workspace.project-manifest-reader'
import pFilter from 'p-filter'
import { glob } from 'tinyglobby'

const DEFAULT_IGNORE = [
  '**/node_modules/**',
  '**/bower_components/**',
  '**/test/**',
  '**/tests/**',
]

// Ordered by precedence. When several of these coexist in one directory, the
// first one present wins. Matches the order tried by `tryReadProjectManifest`.
const MANIFEST_FILENAMES_BY_PRECEDENCE = [
  'package.json',
  'package.json5',
  'package.yaml',
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
  const patterns = normalizePatterns(opts.patterns ?? ['.', '**'])
  delete globOpts.patterns
  const paths: string[] = await glob(patterns, globOpts)

  if (opts.includeRoot) {
    // Always include the workspace root (https://github.com/pnpm/pnpm/issues/1986)
    paths.push(...(await glob(normalizePatterns(['.']), globOpts)))
  }

  const manifestPaths = pickManifestPerDirectory(paths.map(manifestPath => path.join(root, manifestPath)))
  manifestPaths.sort((path1, path2) => lexCompare(path.dirname(path1), path.dirname(path2)))

  return pFilter(
    manifestPaths.map(async manifestPath => {
      try {
        const rootDir = path.dirname(manifestPath) as ProjectRootDir
        return {
          rootDir,
          rootDirRealPath: await fs.realpath(rootDir) as ProjectRootDirRealPath,
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

/**
 * The glob matches every manifest file in a directory, so a directory holding
 * both package.json and package.json5 would otherwise yield two projects with
 * the same rootDir. Keep one manifest per directory.
 */
function pickManifestPerDirectory (manifestPaths: string[]): string[] {
  const byDir = new Map<string, Map<string, string>>()
  for (const manifestPath of manifestPaths) {
    const fileName = path.basename(manifestPath)
    if (!MANIFEST_FILENAMES_BY_PRECEDENCE.includes(fileName)) continue
    const dir = path.dirname(manifestPath)
    let byFileName = byDir.get(dir)
    if (byFileName == null) {
      byFileName = new Map()
      byDir.set(dir, byFileName)
    }
    byFileName.set(fileName, manifestPath)
  }
  const selected: string[] = []
  for (const byFileName of byDir.values()) {
    for (const fileName of MANIFEST_FILENAMES_BY_PRECEDENCE) {
      const manifestPath = byFileName.get(fileName)
      if (manifestPath != null) {
        selected.push(manifestPath)
        break
      }
    }
  }
  return selected
}

function normalizePatterns (patterns: readonly string[]): string[] {
  const normalizedPatterns: string[] = []
  for (const pattern of patterns) {
    normalizedPatterns.push(pattern.replace(/\/?$/, '/package.{json,yaml,json5}'))
  }
  return normalizedPatterns
}

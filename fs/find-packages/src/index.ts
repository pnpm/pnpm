import { promises as fs } from 'fs'
import path from 'path'
import util from 'util'
import { readExactProjectManifest } from '@pnpm/read-project-manifest'
import { type Project, type ProjectRootDir, type ProjectRootDirRealPath } from '@pnpm/types'
import { lexCompare } from '@pnpm/util.lex-comparator'
import { glob } from 'tinyglobby'
import pFilter from 'p-filter'

const DEFAULT_IGNORE = [
  '**/node_modules/**',
  '**/bower_components/**',
  '**/test/**',
  '**/tests/**',
]

export interface Options {
  ignore?: string[]
  includeRoot?: boolean
  patterns?: string[]
}

export async function findPackages (root: string, opts?: Options): Promise<Project[]> {
  opts = opts ?? {}
  const globOpts = { ...opts, cwd: root, expandDirectories: false }
  globOpts.ignore = opts.ignore ?? DEFAULT_IGNORE
  const patterns = normalizePatterns(opts.patterns ?? ['.', '**'])
  delete globOpts.patterns
  const paths: string[] = await glob(patterns, globOpts)

  if (opts.includeRoot) {
    // Always include the workspace root (https://github.com/pnpm/pnpm/issues/1986)
    Array.prototype.push.apply(
      paths,
      await glob(normalizePatterns(['.']), globOpts)
    )
  }

  return pFilter(
    // `Array.from()` doesn't create an intermediate instance,
    // unlike `array.map()`
    Array.from(
      // Remove duplicate paths using `Set`
      new Set(
        paths
          .map(manifestPath => path.join(root, manifestPath))
          .sort((path1, path2) =>
            lexCompare(path.dirname(path1), path.dirname(path2))
          )
      ),
      async manifestPath => {
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

function normalizePatterns (patterns: readonly string[]): string[] {
  const normalizedPatterns: string[] = []
  for (let pattern of patterns) {
    // For some reason tinyglobby hangs indefinitely with !/
    if (pattern.startsWith('!/')) {
      pattern = `!${pattern.substring(2)}`
    }
    // We should add separate pattern for each extension
    // for some reason, fast-glob is buggy with /package.{json,yaml,json5} pattern
    normalizedPatterns.push(
      pattern.replace(/\/?$/, '/package.json')
    )
    normalizedPatterns.push(
      pattern.replace(/\/?$/, '/package.json5')
    )
    normalizedPatterns.push(
      pattern.replace(/\/?$/, '/package.yaml')
    )
  }
  return normalizedPatterns
}

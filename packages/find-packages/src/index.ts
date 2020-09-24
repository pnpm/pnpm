import { readExactProjectManifest } from '@pnpm/read-project-manifest'
import { ProjectManifest } from '@pnpm/types'
import path = require('path')
import fastGlob = require('fast-glob')
import pFilter = require('p-filter')

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

export interface Project {
  dir: string
  manifest: ProjectManifest

  writeProjectManifest: (manifest: ProjectManifest, force?: boolean | undefined) => Promise<void>
}

export default async function findPkgs (root: string, opts?: Options): Promise<Project[]> {
  opts = opts ?? {}
  const globOpts = { ...opts, cwd: root, includeRoot: undefined }
  globOpts.ignore = opts.ignore ?? DEFAULT_IGNORE
  const patterns = normalizePatterns(opts.patterns ? opts.patterns : ['.', '**'])
  const paths: string[] = await fastGlob(patterns, globOpts)

  if (opts.includeRoot) {
    // Always include the workspace root (https://github.com/pnpm/pnpm/issues/1986)
    Array.prototype.push.apply(
      paths,
      await fastGlob(normalizePatterns(['.']), globOpts)
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
            path.dirname(path1).localeCompare(path.dirname(path2))
          )
      ),
      async manifestPath => {
        try {
          return {
            dir: path.dirname(manifestPath),
            ...await readExactProjectManifest(manifestPath),
          } as Project
        } catch (err) {
          if (err.code === 'ENOENT') {
            return null!
          }
          throw err
        }
      }),
    Boolean
  )
}

function normalizePatterns (patterns: readonly string[]) {
  const normalizedPatterns: string[] = []
  for (const pattern of patterns) {
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

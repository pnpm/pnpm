import { readExactImporterManifest } from '@pnpm/read-importer-manifest'
import fastGlob = require('fast-glob')
import pFilter = require('p-filter')
import path = require('path')

const DEFAULT_IGNORE = [
  '**/node_modules/**',
  '**/bower_components/**',
  '**/test/**',
  '**/tests/**',
]

async function findPkgs (
  root: string,
  opts?: {
    ignore?: string[],
    patterns?: string[],
  },
) {
  opts = opts || {}
  const globOpts = {...opts, cwd: root }
  globOpts.ignore = opts.ignore || DEFAULT_IGNORE
  globOpts.patterns = opts.patterns
    ? normalizePatterns(opts.patterns)
    : ['**/package.{json,yaml,json5}']

  const paths: string[] = await fastGlob(globOpts.patterns, globOpts)

  return pFilter(
    paths
      .sort()
      .map((manifestPath) => path.join(root, manifestPath))
      .map(async (manifestPath) => {
        try {
          return {
            path: path.dirname(manifestPath),
            ...await readExactImporterManifest(manifestPath),
          }
        } catch (err) {
          if (err.code === 'ENOENT') {
            return null
          }
          throw err
        }
      }),
    Boolean,
  )
}

function normalizePatterns (patterns: string[]) {
  return patterns.map((pattern) => pattern.replace(/\/?$/, '/package.{json,yaml,json5}'))
}

// for backward compatibility
findPkgs['default'] = findPkgs // tslint:disable-line

export = findPkgs

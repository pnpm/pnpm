import { promises as fs } from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { logger } from '@pnpm/logger'
import type { ManifestFormat, Project, ProjectRootDir, ProjectRootDirRealPath } from '@pnpm/types'
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

const DEFAULT_FORMAT_ORDER: readonly ManifestFormat[] = ['json', 'json5', 'yaml']

const FILENAME_TO_FORMAT: Record<string, ManifestFormat> = {
  'package.json': 'json',
  'package.json5': 'json5',
  'package.yaml': 'yaml',
}

export interface FindPackagesOptions {
  ignore?: string[]
  includeRoot?: boolean
  patterns?: string[]
  preferredManifestFormat?: ManifestFormat
}

export async function findPackages (root: string, opts?: FindPackagesOptions): Promise<Project[]> {
  opts = opts ?? {}
  const globOpts = { ...opts, cwd: root, expandDirectories: false }
  globOpts.ignore = opts.ignore ?? DEFAULT_IGNORE
  const patterns = normalizePatterns(opts.patterns ?? ['.', '**'])
  delete globOpts.patterns
  delete (globOpts as { preferredManifestFormat?: ManifestFormat }).preferredManifestFormat
  const paths: string[] = await glob(patterns, globOpts)

  if (opts.includeRoot) {
    // Always include the workspace root (https://github.com/pnpm/pnpm/issues/1986)
    paths.push(...(await glob(normalizePatterns(['.']), globOpts)))
  }

  const selectedManifestPaths = pickManifestPerDirectory(
    Array.from(new Set(paths.map(manifestPath => path.join(root, manifestPath)))),
    opts.preferredManifestFormat
  )
  selectedManifestPaths.sort((path1, path2) =>
    lexCompare(path.dirname(path1), path.dirname(path2))
  )

  return pFilter(
    Array.from(
      selectedManifestPaths,
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
      }
    ),
    Boolean
  )
}

function pickManifestPerDirectory (manifestPaths: string[], preferredFormat?: ManifestFormat): string[] {
  const order: ManifestFormat[] = preferredFormat && DEFAULT_FORMAT_ORDER.includes(preferredFormat)
    ? [preferredFormat, ...DEFAULT_FORMAT_ORDER.filter(f => f !== preferredFormat)]
    : [...DEFAULT_FORMAT_ORDER]
  const byDir = new Map<string, Map<ManifestFormat, string>>()
  for (const manifestPath of manifestPaths) {
    const format = FILENAME_TO_FORMAT[path.basename(manifestPath)]
    if (!format) continue
    const dir = path.dirname(manifestPath)
    let formatMap = byDir.get(dir)
    if (!formatMap) {
      formatMap = new Map()
      byDir.set(dir, formatMap)
    }
    formatMap.set(format, manifestPath)
  }
  const selected: string[] = []
  for (const [dir, formatMap] of byDir) {
    let chosen: { format: ManifestFormat, manifestPath: string } | undefined
    for (const format of order) {
      const manifestPath = formatMap.get(format)
      if (manifestPath != null) {
        chosen = { format, manifestPath }
        break
      }
    }
    if (!chosen) continue
    if (formatMap.size > 1 && preferredFormat != null && chosen.format !== preferredFormat) {
      // Multiple manifest files coexist and the preferred format is not among
      // them — the user's stub manifest may be shadowing the intended one.
      const present = Array.from(formatMap.keys()).map(f => `package.${f}`).join(', ')
      logger.warn({
        message: `Preferred manifest format "${preferredFormat}" not found in "${dir}". Found ${present}; using "${path.basename(chosen.manifestPath)}".`,
        prefix: dir,
      })
    }
    selected.push(chosen.manifestPath)
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

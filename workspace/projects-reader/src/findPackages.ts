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

// Ordered by precedence. When several of these coexist in one directory, the
// first one present wins. Matches the order tried by `tryReadProjectManifest`.
const DEFAULT_FORMAT_ORDER: readonly ManifestFormat[] = ['json', 'json5', 'yaml']

const MANIFEST_FILENAMES_BY_FORMAT: Record<ManifestFormat, string> = {
  json: 'package.json',
  json5: 'package.json5',
  yaml: 'package.yaml',
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
  const paths: string[] = await glob(patterns, globOpts)

  if (opts.includeRoot) {
    // Always include the workspace root (https://github.com/pnpm/pnpm/issues/1986)
    paths.push(...(await glob(normalizePatterns(['.']), globOpts)))
  }

  const manifestPaths = pickManifestPerDirectory(
    paths.map(manifestPath => path.join(root, manifestPath)),
    opts.preferredManifestFormat
  )
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
 * the same rootDir. Keep one manifest per directory, preferring the requested
 * format when it is present.
 */
function pickManifestPerDirectory (manifestPaths: string[], preferredFormat?: ManifestFormat): string[] {
  const order = buildFormatOrder(preferredFormat)
  const byDir = new Map<string, Map<ManifestFormat, string>>()
  for (const manifestPath of manifestPaths) {
    const format = formatOfManifestFile(path.basename(manifestPath))
    if (format == null) continue
    const dir = path.dirname(manifestPath)
    let byFormat = byDir.get(dir)
    if (byFormat == null) {
      byFormat = new Map()
      byDir.set(dir, byFormat)
    }
    byFormat.set(format, manifestPath)
  }
  const selected: string[] = []
  for (const [dir, byFormat] of byDir) {
    for (const format of order) {
      const manifestPath = byFormat.get(format)
      if (manifestPath == null) continue
      if (preferredFormat != null && format !== preferredFormat && byFormat.size > 1) {
        warnAboutShadowedManifest(dir, byFormat, preferredFormat, manifestPath)
      }
      selected.push(manifestPath)
      break
    }
  }
  return selected
}

function buildFormatOrder (preferredFormat?: ManifestFormat): readonly ManifestFormat[] {
  if (preferredFormat == null || !DEFAULT_FORMAT_ORDER.includes(preferredFormat)) {
    return DEFAULT_FORMAT_ORDER
  }
  return [preferredFormat, ...DEFAULT_FORMAT_ORDER.filter(format => format !== preferredFormat)]
}

function formatOfManifestFile (fileName: string): ManifestFormat | undefined {
  return DEFAULT_FORMAT_ORDER.find(format => MANIFEST_FILENAMES_BY_FORMAT[format] === fileName)
}

/**
 * Only warn when several manifests coexist and the preferred one is absent,
 * which is the surprising case: a stub manifest shadowing the intended one.
 * A directory part-way through a migration holds one file and stays quiet.
 */
function warnAboutShadowedManifest (
  dir: string,
  byFormat: Map<ManifestFormat, string>,
  preferredFormat: ManifestFormat,
  chosenManifestPath: string
): void {
  const present = Array.from(byFormat.keys(), format => MANIFEST_FILENAMES_BY_FORMAT[format]).join(', ')
  logger.warn({
    message: `The preferred manifest format "${preferredFormat}" was not found in "${dir}". Found ${present}; using "${path.basename(chosenManifestPath)}".`,
    prefix: dir,
  })
}

function normalizePatterns (patterns: readonly string[]): string[] {
  const normalizedPatterns: string[] = []
  for (const pattern of patterns) {
    normalizedPatterns.push(pattern.replace(/\/?$/, '/package.{json,yaml,json5}'))
  }
  return normalizedPatterns
}

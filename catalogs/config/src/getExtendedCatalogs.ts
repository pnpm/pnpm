import fs from 'node:fs'
import path from 'node:path'

import type { Catalogs } from '@pnpm/catalogs.types'
import { PnpmError } from '@pnpm/error'
import { readWorkspaceManifest, type WorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'
import { glob } from 'tinyglobby'

import { getCatalogsFromWorkspaceManifest } from './getCatalogsFromWorkspaceManifest.js'
import { mergeCatalogs } from './mergeCatalogs.js'

type ExtendableManifest = Pick<WorkspaceManifest, 'catalog' | 'catalogs' | 'extends'>

const WORKSPACE_MANIFEST_FILENAME = 'pnpm-workspace.yaml'

/**
 * Placeholder, usable as a path prefix in `extends`, that resolves to the
 * monorepo root: the nearest ancestor directory (above the extending manifest)
 * that contains a `pnpm-workspace.yaml`. It lets a manifest reference the root
 * without counting `../` segments, e.g. `<root>`, `<root>/configs/base`.
 */
const ROOT_TOKEN = '<root>'

interface ExtendsTarget {
  /** Directory that contains the `pnpm-workspace.yaml` to merge. */
  dir: string
  /** Whether the target came from a glob (missing manifests are skipped, not an error). */
  fromGlob: boolean
}

/**
 * Resolves the catalogs of a workspace manifest, merging in the catalogs of the
 * workspace manifests it references through the `extends` field.
 *
 * Each `extends` entry may be:
 * - a directory that contains a `pnpm-workspace.yaml` (relative or absolute,
 *   inside or outside the workspace),
 * - a direct path to a `pnpm-workspace.yaml` file,
 * - a glob such as `packages/*` (expanded to every matching `pnpm-workspace.yaml`;
 *   matches without one are skipped), or
 * - a path prefixed with the `<root>` token, which expands to the monorepo root.
 *
 * The merge is performed so that the manifest doing the extending wins on
 * conflicts. When several manifests are extended, entries from manifests
 * resolved later override entries from manifests resolved earlier (glob matches
 * are ordered lexicographically).
 *
 * `extends` is resolved recursively, so an extended manifest may extend other
 * manifests too. Circular references are detected and reported as errors.
 */
export async function getExtendedCatalogs (
  dir: string,
  manifest: ExtendableManifest | undefined
): Promise<Catalogs> {
  return resolveExtendedCatalogs(dir, manifest, [])
}

async function resolveExtendedCatalogs (
  dir: string,
  manifest: ExtendableManifest | undefined,
  ancestors: readonly string[]
): Promise<Catalogs> {
  const ownCatalogs = getCatalogsFromWorkspaceManifest(manifest)
  const extendsEntries = normalizeExtends(manifest?.extends)
  if (extendsEntries.length === 0) {
    return ownCatalogs
  }

  const currentDir = path.resolve(dir)
  if (ancestors.includes(currentDir)) {
    throw new PnpmError(
      'WORKSPACE_EXTENDS_CYCLE',
      `Circular workspace "extends" reference detected. The workspace at "${currentDir}" eventually extends itself.`
    )
  }
  const nextAncestors = [...ancestors, currentDir]

  const targetsPerEntry = await Promise.all(
    extendsEntries.map(async (entry) => resolveExtendsEntry(entry, currentDir))
  )
  const targets = targetsPerEntry.flat()

  const extendedCatalogsList = await Promise.all(targets.map(async (target) => {
    const extendedManifest = await readWorkspaceManifest(target.dir)
    if (extendedManifest == null) {
      // A glob simply matched fewer manifests; only an explicit reference to a
      // missing manifest is an error.
      if (target.fromGlob) return undefined
      throw new PnpmError(
        'WORKSPACE_EXTENDS_NOT_FOUND',
        `Cannot find a ${WORKSPACE_MANIFEST_FILENAME} file in "${target.dir}", which is referenced by the "extends" field of the workspace at "${currentDir}".`
      )
    }
    return resolveExtendedCatalogs(target.dir, extendedManifest, nextAncestors)
  }))

  // The extending manifest's own catalogs are merged last so they take
  // precedence over the catalogs coming from the extended manifests.
  return mergeCatalogs(...extendedCatalogsList, ownCatalogs)
}

async function resolveExtendsEntry (entry: string, currentDir: string): Promise<ExtendsTarget[]> {
  const expanded = expandRootToken(entry, currentDir)
  if (isGlobPattern(expanded)) {
    const dirs = await globManifestDirs(expanded, currentDir)
    return dirs.map((dir) => ({ dir, fromGlob: true }))
  }
  const resolved = path.resolve(currentDir, expanded)
  const dir = path.basename(resolved) === WORKSPACE_MANIFEST_FILENAME
    ? path.dirname(resolved)
    : resolved
  return [{ dir, fromGlob: false }]
}

/**
 * Expands a leading `<root>` token to the monorepo-root path. Anything after
 * the token is kept as a path suffix, so `<root>/configs/base` points at
 * `configs/base` under the root.
 */
function expandRootToken (entry: string, currentDir: string): string {
  if (entry !== ROOT_TOKEN && !entry.startsWith(`${ROOT_TOKEN}/`)) {
    return entry
  }
  const rootDir = findMonorepoRoot(currentDir)
  const rest = entry.slice(ROOT_TOKEN.length).replace(/^\/+/, '')
  const rootPosix = rootDir.split(path.sep).join('/')
  return rest.length > 0 ? `${rootPosix}/${rest}` : rootPosix
}

/**
 * Walks up from the extending manifest's directory and returns the nearest
 * ancestor that contains a `pnpm-workspace.yaml` (the manifest's own directory
 * is excluded, since it is the one referencing `<root>`).
 */
function findMonorepoRoot (currentDir: string): string {
  let dir = path.dirname(currentDir)
  let previous = currentDir
  while (dir !== previous) {
    if (fs.existsSync(path.join(dir, WORKSPACE_MANIFEST_FILENAME))) {
      return dir
    }
    previous = dir
    dir = path.dirname(dir)
  }
  throw new PnpmError(
    'WORKSPACE_EXTENDS_ROOT_NOT_FOUND',
    `Cannot resolve "${ROOT_TOKEN}" in the "extends" field of the workspace at "${currentDir}": no ancestor directory contains a ${WORKSPACE_MANIFEST_FILENAME} file.`
  )
}

async function globManifestDirs (expandedPattern: string, currentDir: string): Promise<string[]> {
  const pattern = ensureManifestSuffix(expandedPattern)
  const { base, tail } = splitGlobBase(pattern, currentDir)
  const matches = await glob([tail], {
    cwd: base,
    absolute: true,
    expandDirectories: false,
    dot: false,
    ignore: ['**/node_modules/**'],
  })
  const dirs = matches.map((match) => path.dirname(path.resolve(match)))
  // Deduplicate and order deterministically so later matches win predictably.
  return Array.from(new Set(dirs)).sort()
}

/**
 * Points a glob at `pnpm-workspace.yaml` files rather than directories: a
 * pattern such as `packages/*` is suffixed so it matches the manifest inside
 * each matched directory. A pattern that already ends in the manifest filename
 * is left untouched.
 */
function ensureManifestSuffix (pattern: string): string {
  const normalized = pattern.replace(/\/+$/, '')
  return path.posix.basename(normalized) === WORKSPACE_MANIFEST_FILENAME
    ? normalized
    : `${normalized}/${WORKSPACE_MANIFEST_FILENAME}`
}

/**
 * Splits a glob into a concrete base directory and the dynamic remainder.
 * `path.resolve` consumes any `..` and absolute parts of the static prefix, so
 * the remainder handed to the glob matcher never escapes its `cwd`.
 */
function splitGlobBase (pattern: string, currentDir: string): { base: string, tail: string } {
  const segments = pattern.split('/')
  let firstDynamic = segments.findIndex((segment) => isGlobPattern(segment))
  if (firstDynamic < 0) {
    firstDynamic = segments.length - 1
  }
  const baseSpec = segments.slice(0, firstDynamic).join('/') || '.'
  const tail = segments.slice(firstDynamic).join('/')
  return { base: path.resolve(currentDir, baseSpec), tail }
}

function isGlobPattern (value: string): boolean {
  return /[*?{}[\]]/.test(value)
}

function normalizeExtends (extendsField: string | string[] | undefined): string[] {
  if (extendsField == null) return []
  return Array.isArray(extendsField) ? extendsField : [extendsField]
}

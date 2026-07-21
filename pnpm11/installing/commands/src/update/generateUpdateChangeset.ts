import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { getCatalogsFromWorkspaceManifest } from '@pnpm/catalogs.config'
import { parseCatalogProtocol } from '@pnpm/catalogs.protocol-parser'
import type { Catalogs } from '@pnpm/catalogs.types'
import { createMatcher } from '@pnpm/config.matcher'
import { PnpmError } from '@pnpm/error'
import { globalInfo, globalWarn } from '@pnpm/logger'
import type { Project, ProjectManifest, ProjectRootDir } from '@pnpm/types'
import { lexCompare } from '@pnpm/util.lex-comparator'
import { safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'
import { findWorkspaceProjects } from '@pnpm/workspace.projects-reader'
import { readWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'
import { loadJsonFile } from 'load-json-file'
import { equals } from 'ramda'

export interface CaptureUpdateChangesetContextOptions {
  allProjects?: Project[]
  dir: string
  engineStrict?: boolean
  workspaceDir?: string
  workspacePackagePatterns?: string[]
}

type ProdDepSpecs = Pick<ProjectManifest, 'dependencies' | 'optionalDependencies'>

export interface UpdateChangesetContext {
  workspaceDir: string
  rootDirs: ProjectRootDir[]
  prodDepSpecsBefore: Map<ProjectRootDir, ProdDepSpecs | null>
  catalogsBefore: Catalogs
}

/**
 * Records the production dependency specs of every workspace package and the
 * workspace catalogs as they are on disk, so that `generateUpdateChangeset()`
 * can diff them against the state the update leaves behind. Must be called
 * before the update writes any manifests.
 */
export async function captureUpdateChangesetContext (opts: CaptureUpdateChangesetContextOptions): Promise<UpdateChangesetContext> {
  const workspaceDir = opts.workspaceDir ?? opts.dir
  const projects = opts.allProjects ?? (
    opts.workspaceDir
      ? await findWorkspaceProjects(opts.workspaceDir, { ...opts, patterns: opts.workspacePackagePatterns })
      : []
  )
  const rootDirs = projects.length > 0
    ? projects.map(({ rootDir }) => rootDir)
    : [opts.dir as ProjectRootDir]
  const [prodDepSpecsEntries, workspaceManifest] = await Promise.all([
    Promise.all(rootDirs.map(async (rootDir) => {
      const manifest = await safeReadProjectManifestOnly(rootDir)
      return [rootDir, manifest && pickProdDepSpecs(manifest)] as const
    })),
    readWorkspaceManifest(workspaceDir),
  ])
  return {
    workspaceDir,
    rootDirs,
    prodDepSpecsBefore: new Map(prodDepSpecsEntries),
    catalogsBefore: getCatalogsFromWorkspaceManifest(workspaceManifest),
  }
}

/**
 * Writes a `.changeset/pnpm-update-<suffix>.md` file declaring a patch bump
 * for every workspace package whose published artifact is affected by the
 * update: packages whose `dependencies` or `optionalDependencies` specs
 * changed, and packages consuming a catalog entry whose spec changed in
 * `pnpm-workspace.yaml` (their own manifests keep the `catalog:` specifier, so
 * only the catalog diff reveals them). Resolution-only changes and
 * `devDependencies` changes don't affect what consumers install, so they never
 * produce a changeset.
 */
export async function generateUpdateChangeset (ctx: UpdateChangesetContext): Promise<void> {
  const changesetDir = path.join(ctx.workspaceDir, '.changeset')
  await ensureChangesetDirIsSafe(changesetDir)
  const changesetConfigPath = path.join(changesetDir, 'config.json')
  const changesetConfig = await readChangesetConfig(changesetConfigPath)
  if (changesetConfig == null) {
    globalWarn(`No changeset was generated because ${changesetConfigPath} does not exist`)
    return
  }
  const catalogsAfter = getCatalogsFromWorkspaceManifest(await readWorkspaceManifest(ctx.workspaceDir))
  const changedCatalogEntries = findChangedCatalogEntries(ctx.catalogsBefore, catalogsAfter)
  const isIgnored = createMatcher(Array.isArray(changesetConfig.ignore) ? changesetConfig.ignore : [])
  const affectedPackageNames = (await Promise.all(
    ctx.rootDirs.map(async (rootDir) => {
      const manifest = await safeReadProjectManifestOnly(rootDir)
      if (!manifest?.name || manifest.private || isIgnored(manifest.name)) return undefined
      const prodDepSpecs = pickProdDepSpecs(manifest)
      const affected = !equals(ctx.prodDepSpecsBefore.get(rootDir) ?? {}, prodDepSpecs) ||
        usesChangedCatalogEntry(prodDepSpecs, changedCatalogEntries)
      return affected ? manifest.name : undefined
    })
  )).filter((name) => name != null)
  if (affectedPackageNames.length === 0) {
    globalInfo('No changeset was generated because the update did not change the production dependencies of any workspace package')
    return
  }
  affectedPackageNames.sort(lexCompare)
  await ensureChangesetDirIsSafe(changesetDir)
  const changesetPath = path.join(changesetDir, `pnpm-update-${crypto.randomBytes(4).toString('hex')}.md`)
  await fs.promises.writeFile(changesetPath, formatChangeset(affectedPackageNames), { flag: 'wx' })
  globalInfo(`Generated a changeset at ${changesetPath} declaring a patch bump for: ${affectedPackageNames.join(', ')}`)
}

interface ChangesetConfig {
  ignore?: string[]
}

async function readChangesetConfig (configPath: string): Promise<ChangesetConfig | null> {
  try {
    return await loadJsonFile<ChangesetConfig>(configPath)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw new PnpmError(
      'INVALID_CHANGESET_CONFIG',
      `Failed to read changeset config at ${configPath}: ${util.types.isNativeError(err) ? err.message : String(err)}`,
      { cause: err }
    )
  }
}

async function ensureChangesetDirIsSafe (changesetDir: string): Promise<void> {
  let stat
  try {
    stat = await fs.promises.lstat(changesetDir)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return
    }
    throw new PnpmError(
      'UNSAFE_CHANGESET_DIR',
      `Failed to inspect changeset directory at ${changesetDir}: ${util.types.isNativeError(err) ? err.message : String(err)}`,
      { cause: err }
    )
  }
  if (stat.isSymbolicLink() || !stat.isDirectory()) {
    throw new PnpmError('UNSAFE_CHANGESET_DIR', `Refusing to use changeset directory at ${changesetDir} because it is a symlink or not a directory`)
  }
}

function pickProdDepSpecs (manifest: ProjectManifest): ProdDepSpecs {
  const prodDepSpecs: ProdDepSpecs = {}
  if (manifest.dependencies != null) {
    prodDepSpecs.dependencies = manifest.dependencies
  }
  if (manifest.optionalDependencies != null) {
    prodDepSpecs.optionalDependencies = manifest.optionalDependencies
  }
  return prodDepSpecs
}

function findChangedCatalogEntries (before: Catalogs, after: Catalogs): Map<string, Set<string>> {
  const changedEntries = new Map<string, Set<string>>()
  for (const catalogName of new Set([...Object.keys(before), ...Object.keys(after)])) {
    for (const dependencyName of new Set([
      ...Object.keys(before[catalogName] ?? {}),
      ...Object.keys(after[catalogName] ?? {}),
    ])) {
      if (before[catalogName]?.[dependencyName] !== after[catalogName]?.[dependencyName]) {
        let dependencyNames = changedEntries.get(catalogName)
        if (dependencyNames == null) {
          dependencyNames = new Set()
          changedEntries.set(catalogName, dependencyNames)
        }
        dependencyNames.add(dependencyName)
      }
    }
  }
  return changedEntries
}

function usesChangedCatalogEntry (prodDepSpecs: ProdDepSpecs, changedCatalogEntries: Map<string, Set<string>>): boolean {
  if (changedCatalogEntries.size === 0) return false
  for (const deps of [prodDepSpecs.dependencies, prodDepSpecs.optionalDependencies]) {
    for (const [depName, spec] of Object.entries(deps ?? {})) {
      const catalogName = parseCatalogProtocol(spec)
      if (catalogName != null && changedCatalogEntries.get(catalogName)?.has(depName)) {
        return true
      }
    }
  }
  return false
}

function formatChangeset (packageNames: string[]): string {
  const bumps = packageNames.map((name) => `"${name}": patch`).join('\n')
  return `---\n${bumps}\n---\n\nUpdate dependencies.\n`
}

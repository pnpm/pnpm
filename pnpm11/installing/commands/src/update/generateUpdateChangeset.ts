import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

import { getCatalogsFromWorkspaceManifest } from '@pnpm/catalogs.config'
import { parseCatalogProtocol } from '@pnpm/catalogs.protocol-parser'
import type { Catalogs } from '@pnpm/catalogs.types'
import { createMatcher } from '@pnpm/config.matcher'
import { isError, PnpmError } from '@pnpm/error'
import { globalInfo, globalWarn } from '@pnpm/logger'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import type { Project, ProjectManifest, ProjectRootDir } from '@pnpm/types'
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

type UpdateDepSpecs = Pick<ProjectManifest, 'dependencies' | 'optionalDependencies' | 'peerDependencies'>
type ReleaseType = 'patch' | 'major'

export interface UpdateChangesetContext {
  workspaceDir: string
  rootDirs: ProjectRootDir[]
  depSpecsBefore: Map<ProjectRootDir, UpdateDepSpecs | null>
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
  const [depSpecsEntries, workspaceManifest] = await Promise.all([
    Promise.all(rootDirs.map(async (rootDir) => {
      const manifest = await safeReadProjectManifestOnly(rootDir)
      return [rootDir, manifest && pickUpdateDepSpecs(manifest)] as const
    })),
    readWorkspaceManifest(workspaceDir),
  ])
  return {
    workspaceDir,
    rootDirs,
    depSpecsBefore: new Map(depSpecsEntries),
    catalogsBefore: getCatalogsFromWorkspaceManifest(workspaceManifest),
  }
}

/**
 * Writes a `.changeset/pnpm-update-<suffix>.md` file for every workspace
 * package whose published artifact is affected by the update. Production
 * dependency changes get a patch bump. Peer dependency changes get a major
 * bump because they can invalidate existing consumers. Catalog changes are
 * attributed to every package that consumes the changed entry. Resolution-only
 * changes and `devDependencies` changes never produce a changeset.
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
  const releases = (await Promise.all(
    ctx.rootDirs.map(async (rootDir) => detectRelease({ ctx, rootDir, isIgnored, changedCatalogEntries }))
  )).filter((release) => release != null)
  if (releases.length === 0) {
    globalInfo('No changeset was generated because the update did not change the production or peer dependencies of any workspace package')
    return
  }
  releases.sort((a, b) => lexCompare(a.name, b.name))
  await writeChangeset(changesetDir, releases)
}

interface Release {
  name: string
  type: ReleaseType
}

interface DetectReleaseOptions {
  ctx: UpdateChangesetContext
  rootDir: ProjectRootDir
  isIgnored: (name: string) => boolean
  changedCatalogEntries: Map<string, Set<string>>
}

async function detectRelease ({ ctx, rootDir, isIgnored, changedCatalogEntries }: DetectReleaseOptions): Promise<Release | undefined> {
  const manifest = await safeReadProjectManifestOnly(rootDir)
  if (!manifest?.name || manifest.private || isIgnored(manifest.name)) return undefined
  const depSpecs = pickUpdateDepSpecs(manifest)
  const depSpecsBefore = ctx.depSpecsBefore.get(rootDir)
  const peerDependenciesChanged = dependencyGroupChanged(depSpecsBefore, depSpecs, 'peerDependencies') ||
    usesChangedCatalogEntry([depSpecs.peerDependencies], changedCatalogEntries)
  if (peerDependenciesChanged) return { name: manifest.name, type: 'major' }
  const productionDependenciesChanged = dependencyGroupChanged(depSpecsBefore, depSpecs, 'dependencies') ||
    dependencyGroupChanged(depSpecsBefore, depSpecs, 'optionalDependencies') ||
    usesChangedCatalogEntry([depSpecs.dependencies, depSpecs.optionalDependencies], changedCatalogEntries)
  return productionDependenciesChanged ? { name: manifest.name, type: 'patch' } : undefined
}

async function writeChangeset (changesetDir: string, releases: Release[]): Promise<void> {
  await ensureChangesetDirIsSafe(changesetDir)
  const changesetPath = path.join(changesetDir, `pnpm-update-${crypto.randomBytes(4).toString('hex')}.md`)
  await fs.promises.writeFile(changesetPath, formatChangeset(releases), { flag: 'wx' })
  globalInfo(`Generated a changeset at ${changesetPath} for: ${releases.map(({ name, type }) => `${name} (${type})`).join(', ')}`)
}

interface ChangesetConfig {
  ignore?: string[]
}

async function readChangesetConfig (configPath: string): Promise<ChangesetConfig | null> {
  try {
    return await loadJsonFile<ChangesetConfig>(configPath)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw new PnpmError(
      'INVALID_CHANGESET_CONFIG',
      `Failed to read changeset config at ${configPath}: ${isError(err) ? err.message : String(err)}`,
      { cause: err }
    )
  }
}

async function ensureChangesetDirIsSafe (changesetDir: string): Promise<void> {
  let stat
  try {
    stat = await fs.promises.lstat(changesetDir)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') {
      return
    }
    throw new PnpmError(
      'UNSAFE_CHANGESET_DIR',
      `Failed to inspect changeset directory at ${changesetDir}: ${isError(err) ? err.message : String(err)}`,
      { cause: err }
    )
  }
  if (stat.isSymbolicLink() || !stat.isDirectory()) {
    throw new PnpmError('UNSAFE_CHANGESET_DIR', `Refusing to use changeset directory at ${changesetDir} because it is a symlink or not a directory`)
  }
}

function pickUpdateDepSpecs (manifest: ProjectManifest): UpdateDepSpecs {
  const depSpecs: UpdateDepSpecs = {}
  if (manifest.dependencies != null) {
    depSpecs.dependencies = manifest.dependencies
  }
  if (manifest.optionalDependencies != null) {
    depSpecs.optionalDependencies = manifest.optionalDependencies
  }
  if (manifest.peerDependencies != null) {
    depSpecs.peerDependencies = manifest.peerDependencies
  }
  return depSpecs
}

function dependencyGroupChanged (
  before: UpdateDepSpecs | null | undefined,
  after: UpdateDepSpecs,
  field: keyof UpdateDepSpecs
): boolean {
  return !equals(before?.[field] ?? {}, after[field] ?? {})
}

function findChangedCatalogEntries (before: Catalogs, after: Catalogs): Map<string, Set<string>> {
  const changedEntries = new Map<string, Set<string>>()
  for (const catalogName of new Set([...Object.keys(before), ...Object.keys(after)])) {
    const changedDependencyNames = findChangedDependencyNames(before[catalogName], after[catalogName])
    if (changedDependencyNames.size > 0) {
      changedEntries.set(catalogName, changedDependencyNames)
    }
  }
  return changedEntries
}

function findChangedDependencyNames (before: Catalogs[string], after: Catalogs[string]): Set<string> {
  const changedDependencyNames = new Set<string>()
  for (const dependencyName of new Set([...Object.keys(before ?? {}), ...Object.keys(after ?? {})])) {
    if (before?.[dependencyName] !== after?.[dependencyName]) {
      changedDependencyNames.add(dependencyName)
    }
  }
  return changedDependencyNames
}

function usesChangedCatalogEntry (
  dependencyGroups: Array<ProjectManifest['dependencies']>,
  changedCatalogEntries: Map<string, Set<string>>
): boolean {
  if (changedCatalogEntries.size === 0) return false
  for (const deps of dependencyGroups) {
    for (const [depName, spec] of Object.entries(deps ?? {})) {
      const catalogName = parseCatalogProtocol(spec)
      if (catalogName != null && changedCatalogEntries.get(catalogName)?.has(depName)) {
        return true
      }
    }
  }
  return false
}

function formatChangeset (releases: Release[]): string {
  const bumps = releases.map(({ name, type }) => `${JSON.stringify(name)}: ${type}`).join('\n')
  return `---\n${bumps}\n---\n\nUpdate dependencies.\n`
}

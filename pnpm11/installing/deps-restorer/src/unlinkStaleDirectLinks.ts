import { promises as fs } from 'node:fs'
import path from 'node:path'

import { removeBinsOfDependency, safeReadDependencyManifest } from '@pnpm/bins.remover'
import { rootLogger } from '@pnpm/core-loggers'
import { isError } from '@pnpm/error'
import { readModulesDir } from '@pnpm/fs.read-modules-dir'
import { safeJoinModulesDir } from '@pnpm/fs.symlink-dependency'
import { isProvidedByRoot } from '@pnpm/installing.linking.modules-cleaner'
import type { LockfileObject, ProjectSnapshot } from '@pnpm/lockfile.fs'
import { DEPENDENCIES_FIELDS, type ProjectId } from '@pnpm/types'
import { isStrictlyInside } from '@pnpm/workspace.state'

import type { HeadlessContext } from './context.js'
import { limitLinking, limitModulesDirReads } from './limits.js'
import { holdsAnyOf } from './linkHoistedModules.js'
import { isSymlink, unlinkIfSymlink } from './unlinkIfSymlink.js'

/**
 * Unlinks the direct dependencies of the selected projects that the wanted
 * lockfile no longer records at the same version, or that the workspace root
 * now provides under `dedupeDirectDeps`. The hoisted linker's orphan diff
 * covers only the package directories it imported, so the `link:` and
 * workspace links written beside them need this pass. Real directories are
 * left to the orphan diff.
 *
 * Reports each `link:` dependency it unlinks. `reportDirectDependencyChanges`
 * reports the other removals.
 */
export async function unlinkStaleDirectLinks (
  { currentLockfile, lockfileDir, opts, selectedProjects, wantedLockfile: fullLockfile }: HeadlessContext,
  wantedLockfile: LockfileObject
): Promise<void> {
  // The link pass dedupes only against a selected root.
  const wantedRootDeps = opts.dedupeDirectDeps && selectedProjects.some(({ id }) => id === '.')
    ? mergeDependencies(wantedLockfile.importers['.' as ProjectId])
    : undefined
  const holdsImporter = selectedProjects.some(project => project.pruneDirectDependencies)
    ? holdsAnyOf([lockfileDir, ...Object.keys(fullLockfile.importers).map(id => path.resolve(lockfileDir, id))])
    : undefined
  await Promise.all(selectedProjects.map(async (project) => {
    const wantedImporter = wantedLockfile.importers[project.id]
    if (wantedImporter == null) return
    const wantedDeps = mergeDependencies(wantedImporter)
    await unlinkStaleLinksOfProject(project, {
      currentImporter: currentLockfile?.importers[project.id],
      lockfileDir,
      wantedDeps,
      wantedRootDeps: project.id === '.' ? undefined : wantedRootDeps,
    })
    if (project.pruneDirectDependencies && !holdsImporter?.(project.modulesDir)) {
      await limitModulesDirReads(() => unlinkExtraneousLinks(project, wantedDeps))
    }
  }))
}

async function unlinkStaleLinksOfProject (
  { binsDir, modulesDir, rootDir }: HeadlessContext['selectedProjects'][number],
  opts: {
    currentImporter: ProjectSnapshot | undefined
    lockfileDir: string
    wantedDeps: Record<string, string>
    /** The root's dependencies to dedupe against, when `dedupeDirectDeps` applies to the project. */
    wantedRootDeps: Record<string, string> | undefined
  }
): Promise<void> {
  const isProvidedByWorkspaceRoot = (alias: string, ref: string): boolean => {
    const rootRef = opts.wantedRootDeps?.[alias]
    return rootRef != null && isProvidedByRoot({ ref, rootDir }, { ref: rootRef, rootDir: opts.lockfileDir })
  }
  const staleDeps = Object.entries(mergeDependencies(opts.currentImporter))
    .filter(([alias, ref]) => opts.wantedDeps[alias] !== ref || isProvidedByWorkspaceRoot(alias, ref))
  await Promise.all(staleDeps.map(async ([alias, ref]) => {
    const unlinked = await unlinkDirectLink(modulesDir, alias, binsDir)
    if (unlinked == null || !ref.startsWith('link:')) return
    rootLogger.debug({
      prefix: rootDir,
      removed: { dependencyType: dependencyType(opts.currentImporter, alias), name: alias, version: unlinked.version },
    })
  }))
}

async function unlinkExtraneousLinks (
  { modulesDir, rootDir, binsDir, manifest }: HeadlessContext['selectedProjects'][number],
  wantedDeps: Record<string, string>
): Promise<void> {
  if (!isStrictlyInside(modulesDir, rootDir) || !await isRealDir(modulesDir)) return
  const dir = await fs.realpath(modulesDir)
  if (!isStrictlyInside(dir, await fs.realpath(rootDir))) return
  const declared = new Set(DEPENDENCIES_FIELDS.flatMap(field => Object.keys(manifest[field] ?? {})))
  const extra = (await readModulesDir(dir, { symlinksOnly: true }) ?? []).filter(alias => !Object.hasOwn(wantedDeps, alias) && !declared.has(alias))
  await Promise.all(extra.map(name => limitLinking(async () => {
    const unlinked = await unlinkDirectLink(dir, name, binsDir).catch(ignoreInvalidAlias)
    if (unlinked) rootLogger.debug({ prefix: rootDir, removed: { name, version: unlinked.version } })
  })))
}

function ignoreInvalidAlias (error: unknown): undefined {
  if (isError(error) && 'code' in error && error.code === 'ERR_PNPM_INVALID_DEPENDENCY_NAME') return undefined
  throw error
}

function dependencyType (snapshot: ProjectSnapshot | undefined, alias: string): 'dev' | 'optional' | 'prod' {
  if (snapshot?.devDependencies?.[alias] != null) return 'dev'
  if (snapshot?.optionalDependencies?.[alias] != null) return 'optional'
  return 'prod'
}

function mergeDependencies (snapshot: ProjectSnapshot | undefined): Record<string, string> {
  return Object.assign({}, ...DEPENDENCIES_FIELDS.map((depType) => snapshot?.[depType]))
}

/**
 * Returns `undefined` when there was no link to remove, and otherwise the
 * linked package's manifest version.
 */
async function unlinkDirectLink (modulesDir: string, alias: string, binsDir: string): Promise<{ version?: string } | undefined> {
  const link = safeJoinModulesDir(modulesDir, alias)
  // A symlinked `@scope` directory would redirect the unlink outside the modules directory.
  if (alias.startsWith('@') && !await isRealDir(path.dirname(link))) return undefined
  if (!await isSymlink(link)) return undefined
  // Unlinking through a symlinked `.bin` would delete files outside the project.
  const version = await isRealDir(binsDir)
    ? (await removeBinsOfDependency(link, { binsDir }))?.version
    : (await safeReadDependencyManifest(link))?.version
  await unlinkIfSymlink(link)
  return { version }
}

async function isRealDir (dir: string): Promise<boolean> {
  const stats = await fs.lstat(dir).catch(() => undefined)
  return stats?.isDirectory() === true
}

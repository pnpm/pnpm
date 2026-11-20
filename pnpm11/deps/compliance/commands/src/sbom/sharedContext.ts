import { realpath } from 'node:fs/promises'
import path from 'node:path'
import { isDeepStrictEqual } from 'node:util'

import { readProjectManifestOnly } from '@pnpm/cli.utils'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { isSpdxLicenseExpression, resolveLicenseFromDir } from '@pnpm/deps.compliance.license-resolver'
import { PnpmError } from '@pnpm/error'
import { getLockfileImporterId, readWantedLockfile } from '@pnpm/lockfile.fs'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { getStorePath } from '@pnpm/store.path'
import type { ProjectManifest } from '@pnpm/types'
import { safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'
import pLimit from 'p-limit'

import type { SbomCommandOptions } from './sbom.js'

export type ManifestLike = { name?: string, version?: string, license?: string, description?: string, author?: string | { name?: string }, repository?: string | { url?: string } }

export interface SharedContext {
  directoryRootsByDepPath: Map<string, string>
  lockfile: Exclude<Awaited<ReturnType<typeof readWantedLockfile>>, null>
  rootManifest: Awaited<ReturnType<typeof readProjectManifestOnly>>
  rootManifestDir: string
  /** Workspace-root license, resolved once so split mode does not re-probe the
   * root directory as the fallback for every emitted SBOM. */
  rootLicense: string | undefined
  storeDir: string | undefined
  /** Workspace package manifests keyed by lockfile importer id, read once from the
   * project graph so split mode does not re-read them for every emitted SBOM. */
  workspaceManifestsByImporterId: Map<string, ManifestLike>
  excludePeerNamesByImporter?: Map<string, Set<string>>
}

type ProjectsGraph = NonNullable<SbomCommandOptions['allProjectsGraph']>

export async function buildSharedContext (opts: SbomCommandOptions): Promise<SharedContext> {
  const lockfileDir = opts.lockfileDir ?? opts.workspaceDir ?? opts.dir
  const directoryRootsByDepPath = new Map<string, string>()
  const lockfile = opts.sharedWorkspaceLockfile === false && opts.workspaceDir && !opts.lockfileDir
    ? await readDedicatedWorkspaceLockfiles(opts, lockfileDir, directoryRootsByDepPath)
    : await readWantedLockfile(opts.lockfileDir ?? opts.dir, { ignoreIncompatible: true })

  if (lockfile == null) {
    throw new PnpmError(
      'SBOM_NO_LOCKFILE',
      `No ${WANTED_LOCKFILE} found: Cannot generate SBOM without a lockfile`
    )
  }

  const rootManifestDir = opts.rootProjectManifestDir ?? opts.dir
  const rootManifest = opts.rootProjectManifest ?? await readProjectManifestOnly(rootManifestDir)

  const excludePeerNamesByImporter = opts.excludePeers
    ? await mapPeerNamesByImporter(opts, Object.keys(lockfile.importers), lockfileDir)
    : undefined

  const storeDir = opts.lockfileOnly
    ? undefined
    : await getStorePath({
      pkgRoot: opts.dir,
      storePath: opts.storeDir,
      pnpmHomeDir: opts.pnpmHomeDir,
    })

  const workspaceManifestsByImporterId = mapWorkspaceManifestsByImporterId(opts, lockfileDir)

  const rootLicense = await resolveRootLicense(rootManifest, rootManifestDir)

  return { lockfile, rootManifest, rootManifestDir, rootLicense, storeDir, workspaceManifestsByImporterId, excludePeerNamesByImporter, directoryRootsByDepPath }
}

async function readDedicatedWorkspaceLockfiles (
  opts: SbomCommandOptions, workspaceDir: string, directoryRootsByDepPath: Map<string, string>
): Promise<LockfileObject | null> {
  const projectDirs = selectedAndReachableProjectDirs(opts)
  const limitLockfileReads = pLimit(16)
  const lockfiles = await Promise.all(projectDirs.map((projectDir) =>
    limitLockfileReads(() => readWantedLockfile(projectDir, { ignoreIncompatible: true }))))
  let merged: LockfileObject | null = null
  const missingImporterIds: string[] = []

  for (const [index, projectDir] of projectDirs.entries()) {
    const lockfile = lockfiles[index]
    const importerId = getLockfileImporterId(workspaceDir, projectDir)
    const projectSnapshot = lockfile?.importers[getLockfileImporterId(projectDir, projectDir)]
    if (!lockfile || !projectSnapshot) {
      missingImporterIds.push(importerId)
      continue
    }
    recordDirectoryRoots(merged, lockfile, { projectDir, directoryRootsByDepPath })
    const importers = Object.fromEntries([[importerId, projectSnapshot]]) as LockfileObject['importers']
    if (!merged) {
      merged = { ...lockfile, importers }
      continue
    }
    mergeDedicatedEntries(merged.importers, importers, projectDir, 'importer')
    merged.packages ??= {}
    mergeDedicatedPackages(merged.packages, lockfile.packages ?? {}, projectDir)
  }
  if (merged && missingImporterIds.length > 0) {
    throw new PnpmError(
      'SBOM_MISSING_IMPORTERS',
      `${WANTED_LOCKFILE} has no entry for the selected or reachable workspace project${missingImporterIds.length === 1 ? '' : 's'}: ${missingImporterIds.join(', ')}.`,
      { hint: 'Run "pnpm install" to update it.' }
    )
  }
  return merged
}

function recordDirectoryRoots (
  merged: LockfileObject | null,
  lockfile: LockfileObject,
  { projectDir, directoryRootsByDepPath }: { projectDir: string, directoryRootsByDepPath: Map<string, string> }
): void {
  for (const [depPath, snapshot] of Object.entries(lockfile.packages ?? {})) {
    if (snapshot.resolution?.type !== 'directory') continue
    const existingRoot = directoryRootsByDepPath.get(depPath)
    const existingResolution = merged?.packages?.[depPath as keyof NonNullable<LockfileObject['packages']>]?.resolution
    if (existingRoot && existingResolution && 'directory' in existingResolution && typeof existingResolution.directory === 'string' &&
      path.resolve(existingRoot, existingResolution.directory) !== path.resolve(projectDir, snapshot.resolution.directory)) {
      throw new PnpmError('SBOM_CONFLICTING_LOCKFILE_ENTRIES',
        `Cannot combine dedicated workspace lockfiles because ${projectDir} contains a different package entry for ${depPath}`)
    }
    if (!existingRoot) directoryRootsByDepPath.set(depPath, projectDir)
  }
}

function selectedAndReachableProjectDirs (opts: SbomCommandOptions): string[] {
  const selected = opts.selectedProjectsGraph ?? opts.allProjectsGraph
  if (!selected) return [opts.dir]
  const graph = opts.allProjectsGraph ?? selected
  const dependenciesByDir = new Map(Object.entries(graph).map(([dir, entry]) => [dir, entry.dependencies]))
  const dirs = Object.keys(selected)
  const seen = new Set(dirs)
  for (let index = 0; index < dirs.length; index++) {
    for (const dependencyDir of dependenciesByDir.get(dirs[index]) ?? []) {
      if (!dependenciesByDir.has(dependencyDir) || seen.has(dependencyDir)) continue
      seen.add(dependencyDir)
      dirs.push(dependencyDir)
    }
  }
  return dirs
}

function mergeDedicatedPackages (
  target: NonNullable<LockfileObject['packages']>, incoming: NonNullable<LockfileObject['packages']>, projectDir: string
): void {
  for (const [key, value] of Object.entries(incoming)) {
    if (!Object.hasOwn(target, key)) {
      Object.defineProperty(target, key, { value, enumerable: true, writable: true, configurable: true })
      continue
    }
    const current = target[key as keyof typeof target]
    if (!isDeepStrictEqual({ ...current, optional: undefined }, { ...value, optional: undefined })) {
      throw new PnpmError('SBOM_CONFLICTING_LOCKFILE_ENTRIES',
        `Cannot combine dedicated workspace lockfiles because ${projectDir} contains a different package entry for ${key}`)
    }
    if (!current.optional || !value.optional) delete current.optional
  }
}

function mergeDedicatedEntries (
  target: object, incoming: object, projectDir: string, kind: string
): void {
  const entries = target as Record<string, unknown>
  for (const [key, value] of Object.entries(incoming)) {
    if (Object.hasOwn(entries, key) && !isDeepStrictEqual(entries[key], value)) {
      throw new PnpmError('SBOM_CONFLICTING_LOCKFILE_ENTRIES',
        `Cannot combine dedicated workspace lockfiles because ${projectDir} contains a different ${kind} entry for ${key}`)
    }
    Object.defineProperty(target, key, { value, enumerable: true, writable: true, configurable: true })
  }
}

/**
 * peerDependencies are identified from the manifest, not the lockfile: with
 * auto-install-peers they resolve into the importer's `dependencies` with no
 * distinguishing marker. Map every walked importer to its peer names so the
 * collector can drop them.
 * Keyed by a Map, not a plain object: importer ids come from the lockfile
 * (attacker-controlled in an untrusted clone), and a key like `__proto__`
 * would corrupt a plain object's prototype.
 */
async function mapPeerNamesByImporter (
  opts: SbomCommandOptions,
  lockfileImporterIds: string[],
  lockfileDir: string
): Promise<Map<string, Set<string>>> {
  // Prefer the in-memory project graphs: no extra filesystem reads, and
  // reading from both graphs (not only the selected subset) covers the extra
  // workspace packages collectSbomComponents reaches through `link:` deps, so
  // their peers are filtered too in a filtered run.
  const graphs = [opts.allProjectsGraph, opts.selectedProjectsGraph].filter((graph) => graph != null)
  if (graphs.length > 0) {
    return mapPeerNamesFromGraphs(graphs, lockfileDir)
  }
  // No project graph (e.g. a single-package repo or `--lockfile-only`
  // outside a workspace), so collectSbomComponents walks every importer in
  // the lockfile. Resolve each importer's own manifest from disk so peers in
  // workspace packages are dropped too, not only those in the directory pnpm
  // ran in.
  return readPeerNamesOfImporters(lockfileImporterIds, lockfileDir)
}

function mapPeerNamesFromGraphs (graphs: ProjectsGraph[], lockfileDir: string): Map<string, Set<string>> {
  const byImporter = new Map<string, Set<string>>()
  for (const graph of graphs) {
    for (const [projectDir, { package: project }] of Object.entries(graph)) {
      byImporter.set(getLockfileImporterId(lockfileDir, projectDir), peerNamesFromManifest(project.manifest))
    }
  }
  return byImporter
}

async function readPeerNamesOfImporters (importerIds: string[], lockfileDir: string): Promise<Map<string, Set<string>>> {
  const byImporter = new Map<string, Set<string>>()
  const lockfileRoot = await realpath(lockfileDir)
  // Bound the fan-out: a large workspace can have many importers, and
  // reading every manifest at once would spike open file descriptors.
  const limitManifestReads = pLimit(16)
  await Promise.all(
    importerIds.map((importerId) => limitManifestReads(async () => {
      const importerManifest = await readImporterManifestInsideRoot({ importerId, lockfileDir, lockfileRoot })
      if (importerManifest) {
        byImporter.set(importerId, peerNamesFromManifest(importerManifest))
      }
    }))
  )
  return byImporter
}

/**
 * Returns null (rather than throwing) for an importer whose manifest is gone
 * (e.g. a stale lockfile), and skips the installability check that would
 * otherwise abort the SBOM.
 */
async function readImporterManifestInsideRoot (
  { importerId, lockfileDir, lockfileRoot }: { importerId: string, lockfileDir: string, lockfileRoot: string }
): Promise<ProjectManifest | null> {
  // A crafted lockfile could carry an importer key that escapes the
  // project, via `..` segments or a symlinked directory. Canonicalize
  // with realpath and skip anything resolving outside the project root,
  // so a manifest is never read from outside the tree.
  let importerDir: string
  try {
    importerDir = await realpath(path.resolve(lockfileDir, importerId))
  } catch {
    return null
  }
  const rel = path.relative(lockfileRoot, importerDir)
  if (rel !== '' && (rel.startsWith('..') || path.isAbsolute(rel))) return null
  // safeReadProjectManifestOnly tolerates a missing manifest but still
  // throws on a malformed one (parse error). In an untrusted clone a
  // single junk package.json must not abort the whole SBOM, so skip it
  // (fail-open: an unparseable importer's peers just aren't filtered).
  try {
    return await safeReadProjectManifestOnly(importerDir)
  } catch {
    return null
  }
}

function mapWorkspaceManifestsByImporterId (opts: SbomCommandOptions, lockfileDir: string): Map<string, ManifestLike> {
  const workspaceManifestsByImporterId = new Map<string, ManifestLike>()
  for (const graph of [opts.allProjectsGraph, opts.selectedProjectsGraph]) {
    if (!graph) continue
    for (const [dir, entry] of Object.entries(graph)) {
      workspaceManifestsByImporterId.set(getLockfileImporterId(lockfileDir, dir), entry.package.manifest)
    }
  }
  return workspaceManifestsByImporterId
}

function peerNamesFromManifest (manifest: ProjectManifest): Set<string> {
  // A name declared as both a peer and a regular dependency is a real dependency
  // the package pulls in itself, so keep it.
  const regular = new Set([
    ...Object.keys(manifest.dependencies ?? {}),
    ...Object.keys(manifest.devDependencies ?? {}),
    ...Object.keys(manifest.optionalDependencies ?? {}),
  ])
  return new Set(
    Object.keys(manifest.peerDependencies ?? {}).filter((name) => !regular.has(name))
  )
}

export async function resolveRootLicense (manifest: Parameters<typeof resolveLicenseFromDir>[0]['manifest'], dir: string): Promise<string | undefined> {
  // Skip the on-disk LICENSE probing when the manifest already declares a usable
  // SPDX license; resolveLicenseFromDir would return the same value after the scan.
  if (typeof manifest.license === 'string' && isSpdxLicenseExpression(manifest.license)) {
    return manifest.license
  }
  const info = await resolveLicenseFromDir({ manifest, dir })
  if (info && info.name !== 'Unknown' && (!info.licenseFile || isSpdxLicenseExpression(info.name))) {
    return info.name
  }
  return undefined
}

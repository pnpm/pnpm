import path from 'node:path'

import type { LockfileObject, ProjectSnapshot } from '@pnpm/lockfile.fs'
import {
  DEPENDENCIES_FIELDS,
  type DependenciesField,
  type ProjectId,
  type ProjectManifest,
} from '@pnpm/types'

interface MovedLinkContext {
  projectId: ProjectId
  manifest: ProjectManifest
  /** Old link target mapped to the reconciled one, both lockfile-relative. */
  movedTargets: Map<string, string>
}

/**
 * The lockfile-relative directory that `link:` references to the project
 * point at, following the `publishConfig.directory` and `linkDirectory`
 * rules of the workspace resolver.
 */
export function getLinkTarget (projectId: ProjectId, importer: ProjectSnapshot): string {
  return importer.publishDirectory != null && importer.linkDirectory !== false
    ? path.posix.join(projectId, importer.publishDirectory)
    : path.posix.normalize(projectId)
}

/**
 * Reconciling `publishDirectory` or `linkDirectory` moves where links to a
 * project point. A dependent's `link:` reference that the fold took from the
 * branch lockfile still points at the old target, so the pre-merge reference
 * is restored when it points at the reconciled one.
 */
export function restoreMovedLinks (opts: {
  wantedLockfile: LockfileObject
  preMergeImporters: LockfileObject['importers']
  projects: Array<{ id: ProjectId, manifest: ProjectManifest }>
  mergedLinkTargets: Map<ProjectId, string>
}): void {
  const { wantedLockfile, preMergeImporters, projects, mergedLinkTargets } = opts
  const movedTargets = collectMovedTargets(wantedLockfile, mergedLinkTargets)
  if (movedTargets.size === 0) return
  for (const project of projects) {
    const importer = wantedLockfile.importers[project.id]
    const preMergeImporter = preMergeImporters[project.id]
    if (importer != null && preMergeImporter != null) {
      restoreImporterMovedLinks(importer, preMergeImporter, {
        projectId: project.id,
        manifest: project.manifest,
        movedTargets,
      })
    }
  }
}

function collectMovedTargets (
  wantedLockfile: LockfileObject,
  mergedLinkTargets: Map<ProjectId, string>
): Map<string, string> {
  const movedTargets = new Map<string, string>()
  for (const [projectId, mergedTarget] of mergedLinkTargets) {
    const reconciledTarget = getLinkTarget(projectId, wantedLockfile.importers[projectId])
    if (reconciledTarget !== mergedTarget) {
      movedTargets.set(mergedTarget, reconciledTarget)
    }
  }
  return movedTargets
}

function restoreImporterMovedLinks (
  importer: ProjectSnapshot,
  preMergeImporter: ProjectSnapshot,
  ctx: MovedLinkContext
): void {
  for (const depField of DEPENDENCIES_FIELDS) {
    for (const depName of Object.keys(importer[depField] ?? {})) {
      restoreMovedLink({ importer, preMergeImporter, depField, depName, ctx })
    }
  }
}

function restoreMovedLink (opts: {
  importer: ProjectSnapshot
  preMergeImporter: ProjectSnapshot
  depField: DependenciesField
  depName: string
  ctx: MovedLinkContext
}): void {
  const { importer, preMergeImporter, depField, depName, ctx } = opts
  const preRef = preMergeImporter[depField]?.[depName]
  const preSpec = preMergeImporter.specifiers[depName]
  if (preRef == null || preSpec == null) return
  const expectedSpec = getDeclaredSpec(ctx.manifest, depField, depName) ?? importer.specifiers[depName]
  if (preSpec !== expectedSpec) return
  const deps = importer[depField]!
  if (!isMovedLink(ctx, deps[depName], preRef)) return
  deps[depName] = preRef
  importer.specifiers[depName] = preSpec
}

function isMovedLink (ctx: MovedLinkContext, mergedRef: string, preRef: string): boolean {
  if (mergedRef === preRef || !isLinkRef(mergedRef) || !isLinkRef(preRef)) return false
  const reconciledTarget = ctx.movedTargets.get(resolveLinkRef(ctx.projectId, mergedRef))
  return reconciledTarget === resolveLinkRef(ctx.projectId, preRef)
}

function resolveLinkRef (projectId: ProjectId, ref: string): string {
  return path.posix.join(projectId, ref.slice('link:'.length))
}

export function isLinkRef (ref: string): boolean {
  return ref.startsWith('link:')
}

export function getDeclaredSpec (
  manifest: ProjectManifest,
  depField: DependenciesField,
  depName: string
): string | undefined {
  const fieldDeps = manifest[depField]
  if (fieldDeps != null && Object.hasOwn(fieldDeps, depName)) {
    return fieldDeps[depName]
  }
  const peerDeps = manifest.peerDependencies
  if (depField === 'dependencies' && peerDeps != null && Object.hasOwn(peerDeps, depName)) {
    return peerDeps[depName]
  }
  return undefined
}

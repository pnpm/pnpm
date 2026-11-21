import * as dp from '@pnpm/deps.path'
import type { LockfileObject, PackageSnapshot, ProjectSnapshot } from '@pnpm/lockfile.types'
import type { Resolution, ResolutionPolicyViolation } from '@pnpm/resolving.resolver-base'
import type { DepPath } from '@pnpm/types'

import {
  anchorImporterLink,
  extractLocalPackageTarget,
  importerSpecifierDeclaresLocal,
  isNonRelativeTarget,
  type LocalResolutionPolicy,
  normalizeRelDir,
  policyAuthorizesImporterDep,
} from './verifyLockfileLocalPolicy.js'
import {
  aliasLinkKey,
  expandLocalSnapshotClosure,
  type ImporterScope,
  type IndexedSnapshot,
  isSnapshotBackedByLocalResolution,
  walkFullSnapshotClosure,
} from './verifyLockfileLocalRoots.js'
import {
  fileKeyMismatchesLocalTarget,
  hasLocalFilesystemResolution,
  isLocalGitPackage,
} from './verifyLockfileShape.js'

const LOCAL_SUBDEP_CODE = 'LOCAL_SUBDEP_NOT_ALLOWED'

export interface ImporterSeedContext {
  lockfile: LockfileObject
  policy: LocalResolutionPolicy
  snapshotsByBase: Map<string, IndexedSnapshot[]>
  workspaceDirs: Set<string>
  /**
   * Importers a project's `link:` dependency may target whatever its
   * specifier, as `linkWorkspacePackages`, a `catalog:` entry, or a dedicated
   * lockfile links a range to another project. Unlike `workspaceDirs`, this
   * includes projects absent from the current run.
   */
  lockfileImporterDirs: Set<string>
  importerId: string
}

interface ImporterSeedBuffers {
  allSeed: DepPath[]
  localSeed: DepPath[]
  localGitSeed: DepPath[]
  reachableBaseKeys: Set<string>
  localPackages: Set<string>
  localGitPackages: Set<string>
  linksByAlias: Set<string>
  violations: ResolutionPolicyViolation[]
}

export function buildImporterScope (
  ctx: ImporterSeedContext,
  importer: ProjectSnapshot
): ImporterScope {
  const buffers: ImporterSeedBuffers = {
    allSeed: [],
    localSeed: [],
    localGitSeed: [],
    reachableBaseKeys: new Set<string>(),
    localPackages: new Set<string>(),
    localGitPackages: new Set<string>(),
    linksByAlias: new Set<string>(),
    violations: [],
  }
  const specifiers = importer.specifiers
  for (const depMap of [importer.dependencies, importer.devDependencies, importer.optionalDependencies]) {
    seedImporterDepMap(ctx, depMap, specifiers, buffers)
  }
  const reachableSnapshots = walkFullSnapshotClosure(ctx.lockfile, buffers.allSeed, buffers.reachableBaseKeys)
  expandLocalSnapshotClosure(
    ctx.lockfile,
    ctx.snapshotsByBase,
    { localSeed: buffers.localSeed, localGitSeed: buffers.localGitSeed },
    { localPackages: buffers.localPackages, localGitPackages: buffers.localGitPackages, linksByAlias: buffers.linksByAlias }
  )
  return {
    reachableSnapshots,
    reachableBaseKeys: buffers.reachableBaseKeys,
    localPackages: buffers.localPackages,
    localGitPackages: buffers.localGitPackages,
    linksByAlias: buffers.linksByAlias,
    violations: buffers.violations,
  }
}

function seedImporterDepMap (
  ctx: ImporterSeedContext,
  depMap: Record<string, string> | undefined,
  specifiers: Record<string, string> | undefined,
  buffers: ImporterSeedBuffers
): void {
  if (depMap == null) return
  for (const [alias, ref] of Object.entries(depMap)) {
    const rawSpec = specifiers?.[alias]
    seedSingleImporterDep(ctx, { alias, ref, rawSpec }, buffers)
  }
}

function seedSingleImporterDep (
  ctx: ImporterSeedContext,
  { alias, ref, rawSpec }: { alias: string, ref: string, rawSpec: string | undefined },
  buffers: ImporterSeedBuffers
): void {
  if (ref.startsWith('link:')) {
    const anchored = anchorImporterLink(ctx.importerId, ref.slice(5))
    if (importerLinkIsTrusted(ctx, alias, rawSpec, anchored)) {
      buffers.linksByAlias.add(aliasLinkKey(alias, anchored))
    } else {
      buffers.violations.push({
        name: alias,
        version: rawSpec ?? ref,
        resolution: { type: 'directory', directory: anchored },
        code: LOCAL_SUBDEP_CODE,
        reason: 'an importer dependency declares a non-local specifier but links to an unauthorized local path',
      })
    }
    return
  }
  const childDepPath = dp.refToRelative(ref, alias)
  if (childDepPath == null) return
  const baseKey = dp.removeSuffix(childDepPath)
  buffers.reachableBaseKeys.add(baseKey)
  buffers.allSeed.push(childDepPath)
  classifyImporterResolvedPackage(ctx, { alias, rawSpec }, { childDepPath, baseKey }, buffers)
}

function classifyImporterResolvedPackage (
  ctx: ImporterSeedContext,
  { alias, rawSpec }: { alias: string, rawSpec: string | undefined },
  { childDepPath, baseKey }: { childDepPath: DepPath, baseKey: string },
  buffers: ImporterSeedBuffers
): void {
  const parsed = dp.parse(childDepPath)
  const snap = ctx.lockfile.packages?.[childDepPath] ?? ctx.snapshotsByBase.get(baseKey)?.[0]?.snapshot
  const isLocalFs = isSnapshotBackedByLocalResolution(parsed.nonSemverVersion, snap)
  const isLocalGit = !isLocalFs && isLocalGitPackage(parsed.nonSemverVersion, snap?.resolution)
  if (!isLocalFs && !isLocalGit) return
  if (!importerPkgIsTrusted(ctx, { alias, rawSpec }, { nonSemverVersion: parsed.nonSemverVersion, snapshot: snap })) {
    const resolution = (snap?.resolution as Resolution | undefined) ?? { type: 'directory', directory: parsed.nonSemverVersion ?? childDepPath }
    buffers.violations.push({
      name: parsed.name ?? snap?.name ?? alias,
      version: parsed.nonSemverVersion ?? parsed.version ?? '0.0.0',
      resolution,
      code: LOCAL_SUBDEP_CODE,
      reason: 'an importer dependency declares a non-local specifier but resolves to an unauthorized local package',
    })
    return
  }
  if (isLocalFs) {
    buffers.localPackages.add(baseKey)
    buffers.localSeed.push(childDepPath)
  } else {
    buffers.localGitPackages.add(baseKey)
    buffers.localGitSeed.push(childDepPath)
  }
}

function importerLinkIsTrusted (
  ctx: ImporterSeedContext,
  alias: string,
  rawSpec: string | undefined,
  anchoredTarget: string
): boolean {
  if (ctx.policy.allowDeployFileUrlKeys) return true
  return (rawSpec != null && importerSpecifierDeclaresLocal(ctx.policy, alias, rawSpec)) ||
    ctx.workspaceDirs.has(anchoredTarget) ||
    ctx.lockfileImporterDirs.has(anchoredTarget) ||
    (rawSpec != null && policyAuthorizesImporterDep(ctx, { alias, declaredSpec: rawSpec, target: anchoredTarget }))
}

function importerPkgIsTrusted (
  ctx: ImporterSeedContext,
  { alias, rawSpec }: { alias: string, rawSpec: string | undefined },
  { nonSemverVersion, snapshot }: { nonSemverVersion: string | undefined, snapshot: PackageSnapshot | undefined }
): boolean {
  if (isDeploySnapshot(ctx.policy, nonSemverVersion, snapshot)) return true
  if (rawSpec != null && importerSpecifierDeclaresLocal(ctx.policy, alias, rawSpec)) return true
  if (isWorkspaceDirectorySnapshot(ctx.workspaceDirs, snapshot)) return true
  const target = extractLocalPackageTarget(nonSemverVersion, snapshot)
  return target != null && rawSpec != null && policyAuthorizesImporterDep(ctx, { alias, declaredSpec: rawSpec, target })
}

function isDeploySnapshot (
  policy: LocalResolutionPolicy,
  nonSemverVersion: string | undefined,
  snapshot: PackageSnapshot | undefined
): boolean {
  if (!policy.allowDeployFileUrlKeys || !nonSemverVersion?.startsWith('file://') || snapshot == null) return false
  return hasLocalFilesystemResolution(snapshot.resolution) &&
    !fileKeyMismatchesLocalTarget(nonSemverVersion, snapshot.resolution, {
      allowDeployFileUrlKeys: true,
      lockfileDir: policy.lockfileDir,
    })
}

function isWorkspaceDirectorySnapshot (workspaceDirs: Set<string>, snapshot: PackageSnapshot | undefined): boolean {
  const resolution = snapshot?.resolution as { type?: unknown, directory?: unknown } | undefined
  return resolution?.type === 'directory' &&
    typeof resolution.directory === 'string' &&
    !isNonRelativeTarget(resolution.directory) &&
    workspaceDirs.has(normalizeRelDir(resolution.directory))
}

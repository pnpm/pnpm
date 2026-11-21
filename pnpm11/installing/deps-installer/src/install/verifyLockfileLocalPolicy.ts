import path from 'node:path'

import type { Catalogs } from '@pnpm/catalogs.types'
import { createMatcher } from '@pnpm/config.matcher'
import type { LockfileObject, PackageSnapshot } from '@pnpm/lockfile.types'
import {
  escapesWorkspaceRoot,
  isDriveLetterPrefix,
  isHostFilesystemSpecifier,
  isNonLocalFilesystemSubdepSpec,
} from '@pnpm/resolving.local-resolver'
import type { PackageExtension } from '@pnpm/types'
import semver from 'semver'

import {
  buildSpecTargets,
  collectAuthorizedEdges,
  derefCatalogSpec,
  extractSpecLocalPath,
  relativePosixPath,
} from './verifyLockfileLocalPolicyParse.js'
import {
  canonicalSlashPath,
  decodeFileUrlOrSlashPath,
  extractLocalGitTarget,
} from './verifyLockfileShape.js'

export type LocalDepField = 'dependencies' | 'optionalDependencies' | 'peerDependencies'

export interface AuthorizedLocalEdge {
  parentBaseKey?: string
  parentName?: string
  parentRange?: string
  childAlias: string
  childRange?: string
  targets: string[]
}

export interface LiveWorkspaceProject {
  rootDir: string
  manifest?: {
    publishConfig?: {
      directory?: string
    }
  }
}

export interface LocalResolutionPolicy {
  allowDeployFileUrlKeys?: boolean
  authorizedEdges?: readonly AuthorizedLocalEdge[]
  catalogs?: Catalogs
  extraWorkspaceDirs?: ReadonlySet<string>
  hasAfterAllResolvedHook?: boolean
  hasPnpmfileHook?: boolean
  liveWorkspaceDirs?: ReadonlySet<string>
  lockfileDir?: string
  overridesRootDir?: string
  pristineDeclaredSpecs?: ReadonlyMap<string, Readonly<Record<string, string>>>
  selectedImporterIds?: ReadonlySet<string>
  workspaceDepthAboveLockfile?: number | null
  workspaceDir?: string
}

export interface LocalHookInputs {
  catalogs?: Catalogs
  deploy?: boolean
  extraWorkspaceDirs?: ReadonlySet<string>
  hasAfterAllResolvedHook?: boolean
  hasPnpmfileHook?: boolean
  liveWorkspaceDirs?: ReadonlySet<string>
  lockfileDir?: string
  overrides?: Record<string, string>
  overridesRootDir?: string
  packageExtensions?: Record<string, PackageExtension>
  selectedImporterIds?: ReadonlySet<string>
  workspaceDepthAboveLockfile?: number | null
  workspaceDir?: string
}

export function createLocalResolutionPolicy (inputs: LocalHookInputs): LocalResolutionPolicy {
  const edges = collectAuthorizedEdges(inputs)
  return {
    allowDeployFileUrlKeys: Boolean(inputs.deploy),
    authorizedEdges: edges.length > 0 ? edges : undefined,
    catalogs: inputs.catalogs,
    extraWorkspaceDirs: inputs.extraWorkspaceDirs,
    hasAfterAllResolvedHook: Boolean(inputs.hasAfterAllResolvedHook),
    hasPnpmfileHook: Boolean(inputs.hasPnpmfileHook),
    liveWorkspaceDirs: inputs.liveWorkspaceDirs,
    lockfileDir: inputs.lockfileDir,
    overridesRootDir: inputs.overridesRootDir,
    selectedImporterIds: inputs.selectedImporterIds,
    workspaceDepthAboveLockfile: inputs.workspaceDepthAboveLockfile === undefined ? 0 : inputs.workspaceDepthAboveLockfile,
    workspaceDir: inputs.workspaceDir,
  }
}

export function policyHasAuthorizationSource (policy: LocalResolutionPolicy): boolean {
  return Boolean((policy.authorizedEdges?.length ?? 0) > 0 || policy.hasAfterAllResolvedHook)
}

export function policyHasChildRangedEdges (policy: LocalResolutionPolicy): boolean {
  return Boolean(policy.authorizedEdges?.some((edge) => Boolean(edge.childRange && edge.childRange !== '*')))
}

export function pristineSpecMapKey (baseKey: string, field: LocalDepField): string {
  return `${baseKey}\0${field}`
}

export function appendPristineDeclaredSpecs (
  policy: LocalResolutionPolicy,
  specs: Map<string, Record<string, string>>
): LocalResolutionPolicy {
  if (specs.size === 0) return policy
  return {
    ...policy,
    pristineDeclaredSpecs: specs,
  }
}

export function appendProbedHookEdges (
  policy: LocalResolutionPolicy,
  rewrites: Array<{ parentBaseKey: string, parentName: string, parentRange?: string, childAlias: string, spec: string }>
): LocalResolutionPolicy {
  if (rewrites.length === 0) return policy
  const added: AuthorizedLocalEdge[] = []
  for (const rewrite of rewrites) {
    const targets = buildSpecTargets(rewrite.spec, policy.lockfileDir, policy.lockfileDir)
    if (targets != null) {
      added.push({
        parentBaseKey: rewrite.parentBaseKey,
        parentName: rewrite.parentName,
        parentRange: rewrite.parentRange,
        childAlias: rewrite.childAlias,
        targets,
      })
    }
  }
  if (added.length === 0) return policy
  return {
    ...policy,
    authorizedEdges: [...(policy.authorizedEdges ?? []), ...added],
  }
}

export function normalizeRelDir (dir: string): string {
  const normalized = path.posix.normalize(dir || '.')
  let end = normalized.length
  while (end > 1 && normalized.charCodeAt(end - 1) === 47) end -= 1
  return end === 0 ? '.' : normalized.slice(0, end)
}

export function isNonRelativeTarget (target: string): boolean {
  return target.startsWith('/') ||
    target.includes('\\') ||
    target === '~' ||
    target.startsWith('~/') ||
    isDriveLetterPrefix(target)
}

export function anchorImporterLink (importerId: string, target: string): string {
  return importerId === '.' || isNonRelativeTarget(target)
    ? normalizeRelDir(target)
    : normalizeRelDir(`${importerId}/${target}`)
}

export function escapesAboveDepth (relPath: string, workspaceDepth: number): boolean {
  const anchor = workspaceDepth <= 0 ? '.' : Array.from({ length: workspaceDepth }, () => 'd').join('/')
  return escapesWorkspaceRoot(anchor, relPath)
}

export function targetEscapesWorkspace (policy: LocalResolutionPolicy, relPath: string): boolean {
  if (isNonRelativeTarget(relPath)) {
    if (policy.lockfileDir && policy.workspaceDir && (policy.lockfileDir.startsWith('/') || isDriveLetterPrefix(policy.lockfileDir))) {
      const rel = relativePosixPath(policy.workspaceDir, relPath)
      return rel == null || rel === '..' || rel.startsWith('../')
    }
    return true
  }
  if (policy.workspaceDepthAboveLockfile !== null) {
    return escapesAboveDepth(relPath, policy.workspaceDepthAboveLockfile ?? 0)
  }
  if (!policy.lockfileDir || !policy.workspaceDir) return true
  const joined = `${policy.lockfileDir.replace(/\\/g, '/')}/${relPath}`
  const relToWs = relativePosixPath(policy.workspaceDir, joined)
  return relToWs == null || relToWs === '..' || relToWs.startsWith('../')
}

export function stripFileUrlPrefix (url: string): string | undefined {
  return url.length >= 5 && url.slice(0, 5).toLowerCase() === 'file:' ? url.slice(5) : undefined
}

export function importerSpecifierDeclaresLocal (
  policy: LocalResolutionPolicy,
  alias: string,
  rawSpecifier: string
): boolean {
  const trimmed = rawSpecifier.trim()
  if (trimmed.slice(0, 10).toLowerCase() === 'workspace:') return true
  const resolved = derefCatalogSpec(policy.catalogs, alias, trimmed)
  return resolved.slice(0, 10).toLowerCase() === 'workspace:' ||
    isNonLocalFilesystemSubdepSpec(resolved, () => false) ||
    isHostFilesystemSpecifier(resolved, '.', () => false)
}

export function policyAuthorizesImporterDep (
  policy: LocalResolutionPolicy,
  lockfile: LockfileObject,
  alias: string,
  declaredSpec: string,
  target: string
): boolean {
  if (afterAllResolvedAllows(policy, lockfile, target)) return true
  const resolvedDecl = derefCatalogSpec(policy.catalogs, alias, declaredSpec)
  const normalized = normalizeTargetForMatch(target)
  return (policy.authorizedEdges ?? []).some((edge) => (
    edge.parentBaseKey == null &&
    (edge.parentName == null || edge.parentName === '*') &&
    matchesWildcard(edge.childAlias, alias) &&
    edgeMatchesChildRange(edge, resolvedDecl) &&
    edge.targets.includes(normalized)
  ))
}

export function policyAuthorizesLink (
  policy: LocalResolutionPolicy,
  lockfile: LockfileObject,
  parent: { baseKey: string, name: string, version?: string },
  child: { field: LocalDepField, alias: string, target: string }
): boolean {
  if (afterAllResolvedAllows(policy, lockfile, child.target)) return true
  const declared = lookupPristineDeclaredSpec(policy, parent.baseKey, child.field, child.alias)
  const normalized = normalizeTargetForMatch(child.target)
  return matchingEdges(policy, parent, child.alias, declared).some((edge) => edge.targets.includes(normalized))
}

export function policyAuthorizesPackage (
  policy: LocalResolutionPolicy,
  lockfile: LockfileObject,
  parent: { baseKey: string, name: string, version?: string },
  child: { field: LocalDepField, alias: string, nonSemverVersion?: string, snapshot?: PackageSnapshot }
): boolean {
  const target = extractLocalPackageTarget(child.nonSemverVersion, child.snapshot)
  if (target == null) return false
  if (afterAllResolvedAllows(policy, lockfile, target)) return true
  const declared = lookupPristineDeclaredSpec(policy, parent.baseKey, child.field, child.alias)
  const normalized = normalizeTargetForMatch(target)
  return matchingEdges(policy, parent, child.alias, declared).some((edge) => edge.targets.includes(normalized))
}

export function policyAuthorizesPeerSpec (
  policy: LocalResolutionPolicy,
  lockfile: LockfileObject,
  parent: { baseKey: string, name: string, version?: string },
  alias: string,
  rawSpec: string
): boolean {
  const specPath = extractSpecLocalPath(rawSpec)
  if (specPath == null) return false
  if (afterAllResolvedAllows(policy, lockfile, specPath)) return true
  const declared = lookupPristineDeclaredSpec(policy, parent.baseKey, 'peerDependencies', alias) ??
    derefCatalogSpec(policy.catalogs, alias, rawSpec)
  const normalized = normalizeTargetForMatch(specPath)
  return matchingEdges(policy, parent, alias, declared).some((edge) => edge.targets.includes(normalized))
}

export function computeWorkspaceDepth (
  lockfileDir: string | undefined,
  workspaceDir: string | undefined
): number | null {
  if (!lockfileDir || !workspaceDir) return 0
  const rel = relativePosixPath(workspaceDir, lockfileDir)
  if (rel == null || rel === '..' || rel.startsWith('../')) return null
  if (rel === '' || rel === '.') return 0
  return rel.split('/').filter((segment) => segment !== '' && segment !== '.').length
}

function normalizeTargetForMatch (target: string): string {
  if (target.startsWith('//')) return decodeFileUrlOrSlashPath(target)
  const slash = target.replace(/\\/g, '/')
  if (hasInnerParentSegment(slash)) return `\0${slash}`
  return slash.startsWith('/') || isDriveLetterPrefix(slash) ? canonicalSlashPath(slash) : normalizeRelDir(slash)
}

/**
 * Whether `target` steps up (`..`) after descending into a named segment, as
 * in `linked/../shared`. Lexical normalization collapses such a path, but the
 * filesystem resolves it through `linked`, which may be a symlink.
 */
export function hasInnerParentSegment (target: string): boolean {
  let descended = false
  for (const segment of target.split(/[/\\]/)) {
    if (segment === '' || segment === '.') continue
    if (segment !== '..') {
      descended = true
    } else if (descended) {
      return true
    }
  }
  return false
}

function lookupPristineDeclaredSpec (
  policy: LocalResolutionPolicy,
  baseKey: string,
  field: LocalDepField,
  alias: string
): string | undefined {
  const raw = policy.pristineDeclaredSpecs?.get(pristineSpecMapKey(baseKey, field))?.[alias]
  return raw != null ? derefCatalogSpec(policy.catalogs, alias, raw) : undefined
}

function afterAllResolvedAllows (policy: LocalResolutionPolicy, lockfile: LockfileObject, target: string): boolean {
  if (!policy.hasAfterAllResolvedHook) return false
  const lockfileHasHook = lockfile.pnpmfileChecksum != null || lockfile.untrackedPnpmfileReadPackageHook === true
  return lockfileHasHook && !targetEscapesWorkspace(policy, target)
}

function matchingEdges (
  policy: LocalResolutionPolicy,
  parent: { baseKey: string, name: string, version?: string },
  alias: string,
  declaredChildSpec: string | undefined
): AuthorizedLocalEdge[] {
  return (policy.authorizedEdges ?? []).filter((edge) => (
    (edge.parentBaseKey == null || edge.parentBaseKey === parent.baseKey) &&
    matchesWildcard(edge.childAlias, alias) &&
    edgeMatchesChildRange(edge, declaredChildSpec) &&
    edgeMatchesParent(edge, parent)
  ))
}

function edgeMatchesChildRange (edge: AuthorizedLocalEdge, declaredSpec: string | undefined): boolean {
  const range = edge.childRange
  if (!range || range === '*') return true
  if (declaredSpec == null) return false
  if (declaredSpec === range) return true
  return semver.validRange(range) != null &&
    semver.validRange(declaredSpec) != null &&
    semver.intersects(declaredSpec, range)
}

function edgeMatchesParent (edge: AuthorizedLocalEdge, parent: { name: string, version?: string }): boolean {
  if (edge.parentName == null) return true
  if (!matchesWildcard(edge.parentName, parent.name)) return false
  if (!edge.parentRange) return true
  if (edge.parentRange === '*' || edge.parentRange === parent.version) return true
  return parent.version != null && semver.satisfies(parent.version, edge.parentRange)
}

function matchesWildcard (pattern: string, value: string): boolean {
  if (pattern === '*' || pattern === value) return true
  return pattern.includes('*') && createMatcher(pattern)(value)
}

export function extractLocalPackageTarget (
  nonSemverVersion: string | undefined,
  snapshot: PackageSnapshot | undefined
): string | undefined {
  const resolution = snapshot?.resolution as { type?: unknown, directory?: unknown, tarball?: unknown, repo?: unknown } | undefined
  if (resolution?.type === 'directory' && typeof resolution.directory === 'string') {
    return resolution.directory
  }
  if (resolution?.type === 'git' && typeof resolution.repo === 'string') {
    return extractLocalGitTarget(resolution.repo)
  }
  if (typeof resolution?.tarball === 'string') {
    return stripFileUrlPrefix(resolution.tarball)
  }
  if (nonSemverVersion?.startsWith('file:')) return nonSemverVersion.slice(5)
  return nonSemverVersion != null ? extractLocalGitTarget(nonSemverVersion) : undefined
}

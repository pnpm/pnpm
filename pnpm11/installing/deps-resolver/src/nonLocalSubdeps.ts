import type { VersionOverride } from '@pnpm/config.parse-overrides'
import * as dp from '@pnpm/deps.path'
import { packageRootLinkTarget } from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import { isNonLocalFilesystemSubdepSpec } from '@pnpm/resolving.local-resolver'
import type { PackageResponse } from '@pnpm/store.controller-types'
import type { DependencyManifest, PackageManifest } from '@pnpm/types'
import semver from 'semver'

import type { WantedDependency } from './getWantedDependencies.js'
import {
  assertHoistedPeersAllowed,
  createDetachedPeerSpecChecker,
  type HoistedPeerCheckInput,
  isHoistedPeerAllowed,
  recordLocalPackagePeerSpecs,
  recordNonLocalPeerSpecs,
} from './nonLocalPeerTracking.js'
import { type DependencyRequest, getPkgsInfoFromIds } from './requestDependencyPackage.js'
import type { ResolutionContext } from './resolutionTypes.js'

export {
  assertHoistedPeersAllowed,
  createDetachedPeerSpecChecker,
  type HoistedPeerCheckInput,
  isHoistedPeerAllowed,
}

const CHILD_SPEC_GROUPS = ['dependencies', 'optionalDependencies'] as const
type ChildSpecField = (typeof CHILD_SPEC_GROUPS)[number]

export interface ParentIdentity {
  name?: string
  version?: string
}

/**
 * Whether `pkgResponse` represents a local directory, local tarball, or
 * workspace package whose own manifest may declare relative or host-filesystem
 * dependencies.
 */
export function isLocalPackageResponse (pkgResponse: PackageResponse): boolean {
  if (pkgResponse.body.isLocal) return true
  const rawId = pkgResponse.body.id
  if (packageRootLinkTarget(rawId) != null) return false
  return pkgResponse.body.resolution.type === 'directory' ||
    pkgResponse.body.resolvedVia === 'local-filesystem' ||
    pkgResponse.body.resolvedVia === 'workspace' ||
    rawId.startsWith('file:') ||
    rawId.startsWith('link:')
}

/**
 * Validate a package's manifest after workspace hooks run: reject un-rewritten
 * host-filesystem child dependencies on non-local packages and record peer and
 * hook authorizations for later hoist and edge checks.
 */
export function validateAndRecordNonLocalManifest (
  request: DependencyRequest,
  pkgResponse: PackageResponse,
  hookedManifest: PackageManifest
): void {
  if (isLocalPackageResponse(pkgResponse)) {
    recordLocalPackagePeerSpecs(request.ctx, pkgResponse.body.id, hookedManifest)
    return
  }
  const preHook = pkgResponse.body.unhookedManifest
  if (preHook != null) {
    rejectRetainedLocalSpecs(request, pkgResponse, preHook, hookedManifest)
    recordHookAuthorizedChildren(request.ctx, pkgResponse.body.id, preHook, hookedManifest)
    recordNonLocalPeerSpecs(request.ctx, pkgResponse, preHook, hookedManifest)
    return
  }
  recordHookAuthorizedChildren(request.ctx, pkgResponse.body.id, hookedManifest, hookedManifest)
}

/**
 * Defense-in-depth check on a resolved child edge of a non-local manifest:
 * rejects `local-filesystem` and unauthorized `workspace` resolutions that were
 * not rewritten by workspace hooks or `overrides`.
 */
export function assertLocalSubdepAllowed (
  request: DependencyRequest,
  pkgResponse: PackageResponse
): void {
  const { ctx, options, wantedDependency } = request
  if (options.currentDepth <= 0) return
  if (packageRootLinkTarget(wantedDependency.bareSpecifier) != null) return
  const parentId = options.parentPkg.pkgId
  if (isLocalGitPkgId(parentId) && isGitFileSpec(wantedDependency.bareSpecifier)) return
  const authorized = ctx.hookAuthorizedLocalChildrenByPkgId.get(parentId)
  if (authorized == null) return
  const alias = wantedDependency.alias ?? ''
  if (authorized.has(alias)) return
  if (!isDisallowedChildProvenance(ctx, wantedDependency, pkgResponse.body.resolvedVia, options.currentDepth)) {
    return
  }
  throw createLocalSubdepError(request, parentId, alias, wantedDependency.bareSpecifier)
}

function rejectRetainedLocalSpecs (
  request: DependencyRequest,
  pkgResponse: PackageResponse,
  preHook: DependencyManifest,
  hooked: PackageManifest
): void {
  for (const field of CHILD_SPEC_GROUPS) {
    rejectRetainedFieldSpecs(request, pkgResponse, { preHook, hooked, field })
  }
}

function rejectRetainedFieldSpecs (
  request: DependencyRequest,
  pkgResponse: PackageResponse,
  opts: { preHook: DependencyManifest, hooked: PackageManifest, field: ChildSpecField }
): void {
  const entries = opts.preHook[opts.field]
  if (entries == null) return
  const parent: ParentIdentity = { name: opts.hooked.name, version: opts.hooked.version }
  const localGitParent = isLocalGitParentResponse(pkgResponse)
  for (const [alias, rawSpec] of Object.entries(entries)) {
    if (!isDisallowedManifestEntrySpec(request.ctx, localGitParent, rawSpec)) continue
    if (opts.hooked[opts.field]?.[alias] === rawSpec && !overrideAuthorizesSpec(request.ctx.parsedOverrides, parent, alias, rawSpec)) {
      throw createLocalSubdepError(request, describeNonLocalPackage(pkgResponse.body.id, opts.hooked), alias, rawSpec)
    }
  }
}

function isDisallowedManifestEntrySpec (
  ctx: ResolutionContext,
  localGitParent: boolean,
  rawSpec: unknown
): rawSpec is string {
  if (typeof rawSpec !== 'string') return false
  if (localGitParent && isGitFileSpec(rawSpec)) return false
  return isForbiddenManifestSpec(ctx, rawSpec)
}

function isLocalGitParentResponse (pkgResponse: PackageResponse): boolean {
  const res = pkgResponse.body.resolution as { type?: string, repo?: string }
  return (res.type === 'git' && typeof res.repo === 'string' && /^file:/i.test(res.repo)) ||
    isLocalGitPkgId(pkgResponse.body.id)
}

function isLocalGitPkgId (pkgId: string): boolean {
  const head = dp.removeSuffix(pkgId)
  const atIdx = head.lastIndexOf('@')
  return /^git\+file:/i.test(head) || (atIdx >= 1 && /^git\+file:/i.test(head.slice(atIdx + 1)))
}

function isGitFileSpec (spec: string): boolean {
  const trimmed = spec.trim()
  if (/^git\+file:/i.test(trimmed)) return true
  if (!/^npm:/i.test(trimmed)) return false
  const inner = trimmed.slice(4)
  const atIdx = inner.lastIndexOf('@')
  return /^git\+file:/i.test(inner) || (atIdx >= 1 && /^git\+file:/i.test(inner.slice(atIdx + 1)))
}

export function isForbiddenManifestSpec (ctx: ResolutionContext, specifier: string): boolean {
  if (packageRootLinkTarget(specifier) != null) return false
  return isNonLocalFilesystemSubdepSpec(
    specifier,
    (prefix) => ctx.namedRegistryPrefixes.includes(`${prefix}:`)
  )
}

function recordHookAuthorizedChildren (
  ctx: ResolutionContext,
  pkgId: PackageResponse['body']['id'],
  preHook: DependencyManifest,
  hooked: PackageManifest
): void {
  const authorized = new Set<string>(ctx.hookAuthorizedLocalChildrenByPkgId.get(pkgId))
  collectFieldAuthorizedChildren(ctx, { preHook, hooked, field: 'dependencies' }, authorized)
  collectFieldAuthorizedChildren(ctx, { preHook, hooked, field: 'optionalDependencies' }, authorized)
  ctx.hookAuthorizedLocalChildrenByPkgId.set(pkgId, authorized)
}

function collectFieldAuthorizedChildren (
  ctx: ResolutionContext,
  { preHook, hooked, field }: {
    preHook: DependencyManifest
    hooked: PackageManifest
    field: ChildSpecField
  },
  authorized: Set<string>
): void {
  const entries = hooked[field]
  if (entries == null) return
  const parent: ParentIdentity = { name: hooked.name, version: hooked.version }
  for (const [alias, postSpec] of Object.entries(entries)) {
    if (typeof postSpec !== 'string') continue
    if (preHook[field]?.[alias] !== postSpec || overrideAuthorizesSpec(ctx.parsedOverrides, parent, alias, postSpec)) {
      authorized.add(alias)
    }
  }
}

function isDisallowedChildProvenance (
  ctx: ResolutionContext,
  wantedDependency: WantedDependency,
  resolvedVia: string | undefined,
  currentDepth: number
): boolean {
  if (isForbiddenManifestSpec(ctx, wantedDependency.bareSpecifier)) return true
  if (wantedUsesWorkspaceProtocol(wantedDependency)) return false
  if (resolvedVia === 'local-filesystem') return true
  if (resolvedVia !== 'workspace') return false
  const deepLinksEnabled = ctx.linkWorkspacePackagesDepth === Infinity || currentDepth <= ctx.linkWorkspacePackagesDepth
  return !deepLinksEnabled
}

function wantedUsesWorkspaceProtocol (wantedDependency: WantedDependency): boolean {
  return wantedDependency.bareSpecifier.slice(0, 'workspace:'.length).toLowerCase() === 'workspace:'
}

export function overrideAuthorizesSpec (
  parsedOverrides: readonly VersionOverride[] | undefined,
  parent: ParentIdentity | undefined,
  alias: string,
  specifier: string
): boolean {
  return parsedOverrides?.some((entry) => overrideEntryMatches(entry, parent, alias, specifier)) ?? false
}

function overrideEntryMatches (
  entry: VersionOverride,
  parent: ParentIdentity | undefined,
  alias: string,
  specifier: string
): boolean {
  if (entry.targetPkg.name !== alias || entry.newBareSpecifier !== specifier) return false
  const targetRange = entry.targetPkg.bareSpecifier
  if (targetRange != null && targetRange !== '*' && targetRange !== specifier) return false
  if (entry.parentPkg == null) return true
  if (parent?.name !== entry.parentPkg.name) return false
  return !entry.parentPkg.bareSpecifier ||
    (parent.version != null && semver.satisfies(parent.version, entry.parentPkg.bareSpecifier))
}

export function describeNonLocalPackage (pkgId: string, manifest: PackageManifest): string {
  if (manifest.name && manifest.version) return `${manifest.name}@${manifest.version}`
  if (manifest.name) return manifest.name
  return pkgId
}

function createLocalSubdepError (
  request: DependencyRequest,
  parent: string,
  alias: string,
  specifier: string
): PnpmError {
  return Object.assign(
    new PnpmError(
      'LOCAL_SUBDEP_NOT_ALLOWED',
      `Cannot resolve local filesystem dependency "${specifier}" for "${alias}" declared by non-local package "${parent}"`
    ),
    {
      pkgsStack: getPkgsInfoFromIds(request.options.parentIds, request.ctx.resolvedPkgsById),
    }
  )
}

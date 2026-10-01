import path from 'node:path'

import { hashObject } from '@pnpm/crypto.object-hasher'
import { getPkgIdWithPatchHash, packageRootLinkTarget, refToRelative } from '@pnpm/deps.path'
import type { LockfileObject, LockfileResolution } from '@pnpm/lockfile.types'
import { resolvePlatformSelector, selectPlatformVariant } from '@pnpm/resolving.resolver-base'
import type { DepPath, PkgIdWithPatchHash, SupportedArchitectures } from '@pnpm/types'
import { familySync } from 'detect-libc'

import type { DepsGraph } from './types.js'

export function lockfileToDepGraph (
  lockfile: LockfileObject,
  supportedArchitectures?: SupportedArchitectures,
  lockfileDir?: string
): DepsGraph<DepPath> {
  const graph: DepsGraph<DepPath> = {}
  const linkTargetNodes = new Set<DepPath>()
  if (lockfile.packages != null) {
    for (const [depPath, pkgSnapshot] of Object.entries(lockfile.packages)) {
      const pkgIdWithPatchHash = getPkgIdWithPatchHash(depPath as DepPath)
      const children = lockfileDepsToGraphChildren({
        ...pkgSnapshot.dependencies,
        ...pkgSnapshot.optionalDependencies,
      }, lockfileDir, linkTargetNodes)
      graph[depPath as DepPath] = {
        children,
        pkgIdWithPatchHash,
        resolution: pkgSnapshot.resolution,
        fullPkgId: createFullPkgId(pkgIdWithPatchHash, pkgSnapshot.resolution, supportedArchitectures),
      }
    }
  }
  for (const linkTargetNode of linkTargetNodes) {
    graph[linkTargetNode] = {
      children: {},
      fullPkgId: linkTargetNode,
    }
  }
  return graph
}

function lockfileDepsToGraphChildren (
  deps: Record<string, string>,
  lockfileDir: string | undefined,
  linkTargetNodes: Set<DepPath>
): Record<string, DepPath> {
  const children: Record<string, DepPath> = {}
  for (const [alias, reference] of Object.entries(deps)) {
    const depPath = refToRelative(reference, alias)
    if (depPath) {
      children[alias] = depPath
    } else if (lockfileDir != null && reference.startsWith('link:') && packageRootLinkTarget(reference) == null) {
      const linkTargetNode = `link:${path.resolve(lockfileDir, reference.slice(5))}` as DepPath
      children[alias] = linkTargetNode
      linkTargetNodes.add(linkTargetNode)
    }
  }
  return children
}

export function createFullPkgId (
  pkgIdWithPatchHash: PkgIdWithPatchHash,
  resolution: LockfileResolution,
  supportedArchitectures?: SupportedArchitectures
): string {
  if ('integrity' in resolution && resolution.integrity != null) {
    return `${pkgIdWithPatchHash}:${resolution.integrity}`
  }
  if ('type' in resolution && resolution.type === 'variations') {
    const variantIntegrity = resolveVariantIntegrity(resolution.variants, supportedArchitectures)
    if (variantIntegrity != null) {
      return `${pkgIdWithPatchHash}:${variantIntegrity}`
    }
  }
  return `${pkgIdWithPatchHash}:${hashObject(resolution)}`
}

function resolveVariantIntegrity (
  variants: unknown,
  supportedArchitectures?: SupportedArchitectures
): string | undefined {
  const selector = resolvePlatformSelector(supportedArchitectures, {
    platform: process.platform,
    arch: process.arch,
    libc: familySync(),
  })
  const variant = selectPlatformVariant(variants as Parameters<typeof selectPlatformVariant>[0], selector)
  const chosenResolution = variant?.resolution
  if (chosenResolution && 'integrity' in chosenResolution && chosenResolution.integrity != null) {
    return chosenResolution.integrity as string
  }
  return undefined
}

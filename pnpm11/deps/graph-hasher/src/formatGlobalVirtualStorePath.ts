import { hashObject, hashObjectWithoutSorting } from '@pnpm/crypto.object-hasher'

export function calcLeafGlobalVirtualStorePath (fullPkgId: string, name: string, version: string): string {
  const depsHash = hashObject({ id: fullPkgId, deps: {} })
  const hexDigest = hashObjectWithoutSorting({ engine: null, deps: depsHash }, { encoding: 'hex' })
  return formatGlobalVirtualStorePath(name, version, hexDigest)
}

/**
 * `subdepIds` maps each direct child's alias to its full pkg id
 * (`${name}@${version}:${integrity}`). Each child contributes a leaf hash
 * (no transitive walk) to the parent's hash, so the resulting path differs
 * whenever the set or versions of children change. One level deep only —
 * use calcGraphNodeHash when full graph traversal is needed.
 */
export function calcGlobalVirtualStorePathWithSubdeps (
  fullPkgId: string,
  name: string,
  version: string,
  subdepIds: Record<string, string>
): string {
  const childHashes: Record<string, string> = {}
  for (const [alias, childFullPkgId] of Object.entries(subdepIds)) {
    childHashes[alias] = hashObject({ id: childFullPkgId, deps: {} })
  }
  const depsHash = hashObject({ id: fullPkgId, deps: childHashes })
  const hexDigest = hashObjectWithoutSorting({ engine: null, deps: depsHash }, { encoding: 'hex' })
  return formatGlobalVirtualStorePath(name, version, hexDigest)
}

// Use @/ prefix for unscoped packages to maintain uniform 4-level directory depth
// Scoped: @scope/pkg/version/hash
// Unscoped: @/pkg/version/hash
export function formatGlobalVirtualStorePath (name: string, version: string, hexDigest: string): string {
  // `version` is lockfile-controlled (`pkgSnapshot.version ?? parsed depPath`)
  // and is inserted below as a raw path segment. Every global-virtual-store
  // slot path funnels through here, and callers join the result onto
  // `globalVirtualStoreDir` before passing it to `importPackage`, so a `..`
  // segment in the version would let the slot escape the store root — even
  // when the package name itself is valid (the name's own traversal is caught
  // downstream by `safeJoinModulesDir`). Reject it at this single choke point.
  assertNoPathTraversal(version)
  const prefix = name.startsWith('@') ? '' : '@/'
  return `${prefix}${name}/${version}/${hexDigest}`
}

function assertNoPathTraversal (version: string): void {
  if (version.split(/[/\\]/).includes('..')) {
    const error = new Error(`Refusing to build a virtual-store path with the traversal version segment ${JSON.stringify(version)}`) as Error & { code: string }
    error.code = 'ERR_PNPM_INVALID_DEPENDENCY_NAME'
    throw error
  }
}

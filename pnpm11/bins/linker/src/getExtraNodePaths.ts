/**
 * Returns the `NODE_PATH` entries every bin shim of an install carries: the
 * hidden hoisted modules directory, under the isolated linker with a hoist
 * pattern, unless `extendNodePath` is `false`.
 */
export function getExtraNodePaths (
  { extendNodePath = true, hoistPattern, nodeLinker, hoistedModulesDir }: {
    extendNodePath?: boolean
    hoistPattern?: string[]
    nodeLinker: 'isolated' | 'hoisted' | 'pnp'
    hoistedModulesDir: string
  }
): string[] {
  if (extendNodePath && nodeLinker === 'isolated' && hoistPattern?.length) {
    return [hoistedModulesDir]
  }
  return []
}

/**
 * `NODE_PATH` options for bin shims inside a global virtual store slot. Every
 * project that links the slot shares those shims, so they omit the installing
 * project's `extraNodePaths` and keep only the bin's own dependency
 * directories.
 */
export function getSlotShimNodePathOptions (
  { enableGlobalVirtualStore, extraNodePaths }: {
    enableGlobalVirtualStore?: boolean
    extraNodePaths?: string[]
  }
): { extraNodePaths?: string[], ownNodePaths?: boolean } {
  if (!enableGlobalVirtualStore) return { extraNodePaths }
  return { extraNodePaths: [], ownNodePaths: Boolean(extraNodePaths?.length) }
}

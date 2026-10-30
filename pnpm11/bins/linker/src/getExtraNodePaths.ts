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

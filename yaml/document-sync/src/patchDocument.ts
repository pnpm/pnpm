import yaml from 'yaml'

export interface PatchDocumentOptions {
  /**
   * Updating aliases is inherently ambiguous since they're not a concept in
   * JSON. The default is to unwrap and remove aliases since that's the most
   * correct behavior.
   *
   * Aliases can be configured to "follow", which updates the anchor node.
   * However, this can result in surprising behavior when values in the target
   * JSON between the anchor and the alias don't match.
   *
   * @default 'unwrap'
   */
  readonly aliases?: 'unwrap' | 'follow'
}

interface PatchContext extends PatchDocumentOptions {
  readonly document: yaml.Document
  readonly aliases: 'unwrap' | 'follow'
}

/**
 * Recursively update a YAML document (in-place) to match the contents of a
 * target value.
 *
 * Comments are preserved on a best-effort basis. There are several cases where
 * ambiguity arises. See this package's README.md for details.
 */
export function patchDocument (document: yaml.Document, target: unknown, options?: PatchDocumentOptions): void {
  // Documents with errors can't be stringified and have unpredictable ASTs.
  if (document.errors.length > 0) {
    throw new Error('Document with errors cannot be patched')
  }

  document.contents = patchNode(document.contents, target, {
    document,
    aliases: options?.aliases ?? 'unwrap',
  })
}

function patchNode (node: yaml.Node | null | undefined, target: unknown, ctx: PatchContext): yaml.Node | null {
  if (node == null) {
    return ctx.document.createNode(target)
  }

  if (target == null) {
    return null
  }

  if (yaml.isAlias(node)) {
    return patchAlias(node, target, ctx)
  }

  if (yaml.isScalar(node)) {
    return patchScalar(node, target, ctx)
  }

  if (yaml.isMap(node)) {
    return patchMap(node, target, ctx)
  }

  if (yaml.isSeq(node)) {
    return patchSeq(node, target, ctx)
  }

  const _never: never = node
  throw new Error('Unrecognized yaml node: ' + String(node))
}

function patchAlias (alias: yaml.Alias, target: unknown, ctx: PatchContext): yaml.Node | null {
  const resolved = alias.resolve(ctx.document)

  // This should only happen if the document was corrupted after it was parsed.
  // Unresolved aliases should fail at the parsing stage.
  if (resolved == null) {
    throw new Error('Failed to resolve yaml alias: ' + alias.source)
  }

  switch (ctx.aliases) {
  case 'follow': {
    // This can result in surprising behavior since the anchor node will end up
    // with the contents of the last encountered alias. The default is to
    // "unwrap" for this reason.
    patchNode(resolved, target, ctx)
    return alias
  }

  case 'unwrap': {
    const copy = resolved.clone() as typeof resolved
    copy.anchor = undefined
    patchNode(copy, target, ctx)
    return copy
  }
  }
}

function patchScalar (scalar: yaml.Scalar, target: unknown, ctx: PatchContext): yaml.Node {
  if (scalar.value === target) {
    return scalar
  }

  if (typeof target === 'boolean' || typeof target === 'string' || typeof target === 'number') {
    scalar.value = target
    return scalar
  }

  return ctx.document.createNode(target)
}

function patchMap (map: yaml.YAMLMap, target: unknown, ctx: PatchContext): yaml.Node | null {
  if (!isRecord(target)) {
    return ctx.document.createNode(target)
  }

  // Intentionally return null on empty maps as well. This recursively clears
  // empty maps in the final document.
  if (target == null || Object.keys(target).length === 0) {
    return null
  }

  const mapKeyToExistingPair = new Map<string, yaml.Pair>()

  for (const pair of map.items) {
    // We can't update non-node types. Pairs should only contain values that are
    // non-nodes if the yaml document was modified manually after parsing.
    if (!yaml.isScalar(pair.key) || typeof pair.key.value !== 'string') {
      throw new Error('Encountered unexpected non-node value: ' + String(pair.key))
    }

    mapKeyToExistingPair.set(pair.key.value, pair)
  }

  map.items = Object.entries(target)
    .map(([key, value]) => {
      const existingPair = mapKeyToExistingPair.get(key)

      if (existingPair == null) {
        return ctx.document.createPair(key, value)
      }

      if (!yaml.isNode(existingPair.value)) {
        throw new Error('Encountered unexpected non-node value: ' + String(existingPair.value))
      }

      existingPair.value = patchNode(existingPair.value, value, ctx)
      return existingPair
    })
    .filter((pair) => pair.value != null)

  return map
}

function patchSeq (seq: yaml.YAMLSeq, target: unknown, ctx: PatchContext): yaml.Node {
  if (!Array.isArray(target)) {
    return ctx.document.createNode(target)
  }

  // Primitive lists can benefit from a more correct reconciliation process
  // since it's possible to uniquely identify items.
  //
  // Reconciling lists with objects is more complex since we don't know which
  // objects in the source list semantically correspond to the same object in
  // the target list. This is the same problem that virtual DOM frameworks (e.g.
  // React) have. We have to go by indexes in the complex case. If solving this
  // problem becomes important in the future, it may be worth making callers to
  // pass in a getKeyForNode() function.
  return isPrimitiveList(target)
    ? patchSeqPrimitive(seq, target)
    : patchSeqComplex(seq, target, ctx)
}

function patchSeqPrimitive (seq: yaml.YAMLSeq, target: Array<boolean | number | string | null | undefined>): yaml.Node {
  // Keep track of existing nodes to reuse when building up the final list from
  // the target list. These nodes will have comments attached to them, so it's
  // important to reuse them when possible.
  const valueToNodesMap = new Map<boolean | number | string, yaml.Scalar[]>()

  for (const item of seq.items) {
    if (item != null && !yaml.isNode(item)) {
      throw new Error('Encountered unexpected non-node value: ' + String(item))
    }

    // We know all items in the target list are scalars. If there's a non-scalar
    // in the source list, it needs to be removed. Skip over this item so it's
    // not added to the final list.
    if (!yaml.isScalar(item) || !isPrimitive(item.value) || item.value == null) {
      continue
    }

    const nodeList = valueToNodesMap.get(item.value) ?? []
    nodeList.push(item)

    valueToNodesMap.set(item.value, nodeList)
  }

  seq.items = target.filter(item => item != null).map((item): yaml.Scalar => {
    const existingNodesList = valueToNodesMap.get(item)
    const firstExistingItem = existingNodesList?.shift()

    // If the list is now empty as a result of removing the first item, clean up
    // the map.
    if (existingNodesList?.length === 0) {
      valueToNodesMap.delete(item)
    }

    return firstExistingItem ?? new yaml.Scalar(item)
  })

  return seq
}

function patchSeqComplex (seq: yaml.YAMLSeq, target: unknown[], ctx: PatchContext): yaml.Node {
  const nextItems: yaml.Node[] = []

  for (let i = 0; i < Math.max(seq.items.length, target.length); i++) {
    const existingItem = seq.items[i]
    const targetItem = target[i]

    if (existingItem != null && !yaml.isNode(existingItem)) {
      throw new Error('Encountered unexpected non-node value: ' + String(existingItem))
    }

    const nextItem = patchNode(existingItem, targetItem, ctx)
    if (nextItem == null) {
      continue
    }

    nextItems.push(nextItem)
  }

  seq.items = nextItems
  return seq
}

function isRecord (value: unknown): value is Record<string, unknown> {
  return value != null && typeof value === 'object' && !Array.isArray(value)
}

function isPrimitiveList (arr: unknown[]) {
  return arr.every(isPrimitive)
}

function isPrimitive (value: unknown) {
  return value == null || typeof value === 'boolean' || typeof value === 'string' || typeof value === 'number'
}

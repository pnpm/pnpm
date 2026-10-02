import yaml from 'yaml'

import { preserveScalarAliases } from './preserveScalarAliases.js'

export interface PatchDocumentOptions {
  /** Convert scalar keys to target property names. Defaults to YAML's null-to-empty-string conversion. */
  readonly stringifyKey?: (key: unknown) => string
  /** Keep existing map keys in their original order and append new keys. */
  readonly preserveKeyOrder?: boolean
  /** Remove null values and empty maps. Defaults to true for configuration files. */
  readonly pruneEmptyValues?: boolean
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
  /**
   * Keep scalar aliases whose final values agree, moving removed anchors to a
   * surviving entry. Divergent values become independent scalars. When enabled,
   * this takes precedence over `aliases` for scalar nodes only.
   * @default false
   */
  readonly preserveScalarAliases?: boolean
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

  const restoreAliases = options?.preserveScalarAliases ? preserveScalarAliases(document) : undefined
  document.contents = patchNode(document.contents, target, {
    ...options,
    document,
    aliases: options?.aliases ?? 'unwrap',
  })
  restoreAliases?.()
}

function patchNode (node: yaml.Node | null | undefined, target: unknown, ctx: PatchContext): yaml.Node | null {
  if (node == null) return ctx.document.createNode(target)
  if (target == null) return patchNullTarget(node, ctx)
  return patchNonNullNode(node, target, ctx)
}

function patchNullTarget (node: yaml.Node, ctx: PatchContext): yaml.Node | null {
  if (ctx.pruneEmptyValues !== false) return null
  return yaml.isScalar(node) && node.value == null ? node : ctx.document.createNode(null)
}

function patchNonNullNode (node: yaml.Node, target: unknown, ctx: PatchContext): yaml.Node | null {
  if (yaml.isAlias(node)) return patchAlias(node, target, ctx)
  if (yaml.isScalar(node)) return patchScalar(node, target, ctx)
  if (yaml.isMap(node)) return patchMap(node, target, ctx)
  if (yaml.isSeq(node)) return patchSeq(node, target, ctx)

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
      patchNode(resolved, target, ctx)
      return alias
    }

    case 'unwrap': {
      const copy = resolved.clone() as typeof resolved
      copy.anchor = undefined
      return patchNode(copy, target, ctx)
    }
  }
}

function patchScalar (scalar: yaml.Scalar, target: unknown, ctx: PatchContext): yaml.Node {
  if (scalar.value === target) {
    return scalar
  }

  const replacement = ctx.document.createNode(target)
  if (yaml.isScalar(replacement)) {
    scalar.value = replacement.value
    scalar.tag = replacement.tag
    return scalar
  }

  return replacement
}

function patchMap (map: yaml.YAMLMap, target: unknown, ctx: PatchContext): yaml.Node | null {
  if (!isRecord(target)) return ctx.document.createNode(target)
  if (ctx.pruneEmptyValues !== false && Object.keys(target).length === 0) return null

  const mapKeyToExistingPair = collectExistingPairs(map, ctx)
  const keys = resolveMapKeys(target, mapKeyToExistingPair, ctx.preserveKeyOrder)

  map.items = keys
    .map(key => reconcilePair(key, target[key], mapKeyToExistingPair.get(key), ctx))
    .filter((pair): pair is yaml.Pair => pair != null && pair.value != null)

  return map
}

function collectExistingPairs (map: yaml.YAMLMap, ctx: PatchContext): Map<string, yaml.Pair> {
  const mapKeyToExistingPair = new Map<string, yaml.Pair>()
  for (const pair of map.items) {
    if (!yaml.isScalar(pair.key)) {
      throw new Error('Encountered unexpected non-node value: ' + String(pair.key))
    }
    const keyString = ctx.stringifyKey?.(pair.key.value) ?? String(pair.key.value ?? '')
    mapKeyToExistingPair.set(keyString, pair)
  }
  return mapKeyToExistingPair
}

function resolveMapKeys (
  target: Record<string, unknown>,
  existingPairs: Map<string, yaml.Pair>,
  preserveKeyOrder?: boolean
): string[] {
  if (!preserveKeyOrder) return Object.keys(target)
  const existingInTarget = [...existingPairs.keys()].filter(key => Object.hasOwn(target, key))
  const newInTarget = Object.keys(target).filter(key => !existingPairs.has(key))
  return [...existingInTarget, ...newInTarget]
}

function reconcilePair (
  key: string,
  value: unknown,
  existingPair: yaml.Pair | undefined,
  ctx: PatchContext
): yaml.Pair | null {
  if (existingPair == null) {
    return ctx.document.createPair(key, value)
  }
  if (existingPair.value != null && !yaml.isNode(existingPair.value)) {
    throw new Error('Encountered unexpected non-node value: ' + String(existingPair.value))
  }
  existingPair.value = patchNode(existingPair.value, value, ctx)
  return existingPair
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
    ? patchSeqPrimitive(seq, target, ctx)
    : patchSeqComplex(seq, target, ctx)
}

type PrimitiveItem = boolean | number | string | null | undefined

function patchSeqPrimitive (seq: yaml.YAMLSeq, target: PrimitiveItem[], ctx: PatchContext): yaml.Node {
  const valueToNodesMap = collectPrimitiveNodes(seq, ctx.pruneEmptyValues)

  seq.items = target
    .filter(item => item != null || ctx.pruneEmptyValues === false)
    .map((item): yaml.Scalar => consumeMatchingScalar(item, valueToNodesMap))

  return seq
}

function collectPrimitiveNodes (
  seq: yaml.YAMLSeq,
  pruneEmptyValues?: boolean
): Map<PrimitiveItem, yaml.Scalar[]> {
  const map = new Map<PrimitiveItem, yaml.Scalar[]>()
  for (const item of seq.items) {
    if (item != null && !yaml.isNode(item)) {
      throw new Error('Encountered unexpected non-node value: ' + String(item))
    }
    if (!isValidPrimitiveScalar(item, pruneEmptyValues)) continue
    const nodeList = map.get(item.value) ?? []
    nodeList.push(item)
    map.set(item.value, nodeList)
  }
  return map
}

function isValidPrimitiveScalar (item: unknown, pruneEmptyValues?: boolean): item is yaml.Scalar<PrimitiveItem> {
  if (!yaml.isScalar(item) || !isPrimitive(item.value)) return false
  if (item.value == null && pruneEmptyValues !== false) return false
  return true
}

function consumeMatchingScalar (
  item: PrimitiveItem,
  valueToNodesMap: Map<PrimitiveItem, yaml.Scalar[]>
): yaml.Scalar {
  const existingNodesList = valueToNodesMap.get(item)
  const firstExistingItem = existingNodesList?.shift()
  if (existingNodesList?.length === 0) {
    valueToNodesMap.delete(item)
  }
  return firstExistingItem ?? new yaml.Scalar(item)
}

function patchSeqComplex (seq: yaml.YAMLSeq, target: unknown[], ctx: PatchContext): yaml.Node {
  const nextItems: yaml.Node[] = []

  for (let itemIndex = 0; itemIndex < target.length; itemIndex++) {
    const existingItem = seq.items[itemIndex]
    const targetItem = target[itemIndex]

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

function isPrimitive (value: unknown): value is PrimitiveItem {
  return value == null || typeof value === 'boolean' || typeof value === 'string' || typeof value === 'number'
}

import yaml from 'yaml'

export function preserveScalarAliases (document: yaml.Document): () => void {
  const groups = new Map<yaml.Scalar, yaml.Scalar>()
  const replacements = new Map<yaml.Alias, yaml.Scalar>()
  yaml.visit(document, {
    Scalar (_, node) {
      if (node.anchor) groups.set(node, node)
    },
    Alias (_, node) {
      const source = node.resolve(document)
      if (!yaml.isScalar(source)) return
      const copy = source.clone() as yaml.Scalar
      copy.anchor = undefined
      copy.comment = node.comment
      copy.commentBefore = node.commentBefore
      copy.spaceBefore = node.spaceBefore
      groups.set(copy, source)
      replacements.set(node, copy)
    },
  })
  yaml.visit(document, {
    Alias (_, node) {
      return replacements.get(node)
    },
  })
  return () => {
    assignUniqueNames(document, new Set(groups.values()))
    const anchors = new Map<yaml.Scalar, yaml.Scalar>()
    yaml.visit(document, {
      Scalar (_, node) {
        const source = groups.get(node)
        if (!source) return
        const anchor = anchors.get(source)
        if (!anchor) {
          node.anchor = source.anchor
          anchors.set(source, node)
        } else if (Object.is(anchor.value, node.value)) {
          const alias = new yaml.Alias(anchor.anchor!)
          alias.comment = node.comment
          alias.commentBefore = node.commentBefore
          alias.spaceBefore = node.spaceBefore
          return alias
        }
        return undefined
      },
    })
  }
}

function assignUniqueNames (document: yaml.Document, sources: Set<yaml.Scalar>): void {
  const names = new Map<string, Set<yaml.Node>>()
  yaml.visit(document, {
    Node (_, node) {
      if (yaml.isAlias(node) || !node.anchor) return
      const nodes = names.get(node.anchor) ?? new Set<yaml.Node>()
      nodes.add(node)
      names.set(node.anchor, nodes)
    },
  })
  for (const source of sources) {
    const name = source.anchor!
    const nodes = names.get(name)
    if (!nodes || (nodes.size === 1 && nodes.has(source))) {
      names.set(name, new Set([source]))
      continue
    }
    let suffix = 1
    while (names.has(`${name}_${suffix}`)) suffix++
    source.anchor = `${name}_${suffix}`
    names.set(source.anchor, new Set([source]))
  }
}

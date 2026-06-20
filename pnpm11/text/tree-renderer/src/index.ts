export interface TreeNodeGroup {
  group: string
  nodes: Array<TreeNode | string>
}

export interface TreeNode {
  label: string
  nodes?: Array<TreeNode | string | TreeNodeGroup>
}

export interface TreeRendererOptions {
  /**
   * Formatter applied to tree-drawing character sequences (e.g. `├─┬ `, `│ `).
   * Useful for dimming tree lines so labels stand out: `{ treeChars: chalk.dim }`.
   */
  treeChars?: (chars: string) => string
  /**
   * When false, use ASCII characters (+, `, |, -) instead of
   * unicode box-drawing characters. Defaults to true (unicode).
   */
  unicode?: boolean
}

export function renderTree (node: TreeNode | string, opts?: TreeRendererOptions): string {
  return render(opts ?? {}, { node, connector: '', prefix: '' })
}

interface RenderContext {
  node: TreeNode | string
  /**
   * The formatted connector string for this node's first line
   * (e.g. `├─┬ `). Empty string for the root node.
   */
  connector: string
  /**
   * The raw prefix for subsequent lines and children of this node.
   * Built from unformatted characters so it can be extended for deeper levels.
   */
  prefix: string
}

function render (
  opts: TreeRendererOptions,
  ctx: RenderContext
): string {
  const { connector, prefix } = ctx
  let { node } = ctx
  if (typeof node === 'string') node = { label: node }

  const fmt = opts.treeChars ?? identity
  const chr = opts.unicode === false ? asciiChar : unicodeChar
  const nodes = node.nodes ?? []
  const lines = (node.label || '').split('\n')

  // First line: connector + label
  let result = (connector ? fmt(connector) : '') + lines[0] + '\n'

  // Flatten groups into items with group annotations
  const items: Array<{ node: TreeNode, group?: string }> = []
  for (const child of nodes) {
    if (isGroup(child)) {
      for (const gn of child.nodes) {
        items.push({ node: typeof gn === 'string' ? { label: gn } : gn, group: child.group })
      }
    } else {
      items.push({ node: typeof child === 'string' ? { label: child } : child })
    }
  }

  // Continuation lines for multiline labels
  const continuationChars = items.length ? chr('│') + ' ' : '  '
  for (let l = 1; l < lines.length; l++) {
    result += fmt(prefix + continuationChars) + lines[l] + '\n'
  }

  // Render items, emitting group headers when the group changes
  let currentGroup: string | undefined
  for (let i = 0; i < items.length; i++) {
    const item = items[i]
    const last = i === items.length - 1

    if (item.group !== currentGroup) {
      currentGroup = item.group
      if (currentGroup != null) {
        result += fmt(prefix + chr('│')) + '\n'
        result += fmt(prefix + chr('│') + '   ') + currentGroup + '\n'
      }
    }

    const more = hasRenderableChildren(item.node.nodes)
    const childConnector = prefix +
      (last ? chr('└') : chr('├')) + chr('─') +
      (more ? chr('┬') : chr('─')) + ' '
    const childPrefix = prefix + (last ? '  ' : chr('│') + ' ')

    result += render(opts, {
      node: item.node,
      connector: childConnector,
      prefix: childPrefix,
    })
  }

  return result
}

function hasRenderableChildren (nodes: Array<TreeNode | string | TreeNodeGroup> | undefined): boolean {
  if (nodes == null) return false
  for (const child of nodes) {
    if (isGroup(child)) {
      if (child.nodes.length > 0) return true
    } else {
      return true
    }
  }
  return false
}

function isGroup (node: TreeNode | string | TreeNodeGroup): node is TreeNodeGroup {
  return typeof node !== 'string' && 'group' in node
}

function identity (s: string): string {
  return s
}

function unicodeChar (s: string): string {
  return s
}

function asciiChar (s: string): string {
  const chars: Record<string, string> = {
    '│': '|',
    '└': '`',
    '├': '+',
    '─': '-',
    '┬': '-',
  }
  return chars[s] ?? s
}

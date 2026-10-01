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

interface TreeItem {
  node: TreeNode
  group?: string
}

function render (
  opts: TreeRendererOptions,
  ctx: RenderContext
): string {
  const treeNode = typeof ctx.node === 'string' ? { label: ctx.node } : ctx.node
  const items = flattenItems(treeNode.nodes ?? [])
  const labelOutput = renderLabelLines(treeNode.label || '', ctx, items.length > 0, opts)
  const childrenOutput = renderItems(items, ctx.prefix, opts)
  return labelOutput + childrenOutput
}

function flattenItems (nodes: Array<TreeNode | string | TreeNodeGroup>): TreeItem[] {
  const items: TreeItem[] = []
  for (const child of nodes) {
    if (isGroup(child)) {
      appendGroupItems(items, child)
    } else {
      items.push({ node: typeof child === 'string' ? { label: child } : child })
    }
  }
  return items
}

function appendGroupItems (items: TreeItem[], group: TreeNodeGroup): void {
  for (const groupNode of group.nodes) {
    items.push({
      node: typeof groupNode === 'string' ? { label: groupNode } : groupNode,
      group: group.group,
    })
  }
}

function renderLabelLines (
  label: string,
  ctx: RenderContext,
  hasChildren: boolean,
  opts: TreeRendererOptions
): string {
  const fmt = opts.treeChars ?? identity
  const chr = opts.unicode === false ? asciiChar : unicodeChar
  const lines = label.split('\n')
  let result = (ctx.connector ? fmt(ctx.connector) : '') + lines[0] + '\n'

  const continuationChars = hasChildren ? chr('│') + ' ' : '  '
  for (let lineIndex = 1; lineIndex < lines.length; lineIndex++) {
    result += fmt(ctx.prefix + continuationChars) + lines[lineIndex] + '\n'
  }
  return result
}

function renderItems (
  items: TreeItem[],
  prefix: string,
  opts: TreeRendererOptions
): string {
  let result = ''
  let currentGroup: string | undefined
  for (let itemIndex = 0; itemIndex < items.length; itemIndex++) {
    const item = items[itemIndex]
    const last = itemIndex === items.length - 1
    if (item.group !== currentGroup) {
      currentGroup = item.group
      result += renderGroupHeader(currentGroup, prefix, opts)
    }
    result += renderChildItem(item.node, last, prefix, opts)
  }
  return result
}

function renderGroupHeader (
  group: string | undefined,
  prefix: string,
  opts: TreeRendererOptions
): string {
  if (group == null) return ''
  const fmt = opts.treeChars ?? identity
  const chr = opts.unicode === false ? asciiChar : unicodeChar
  return fmt(prefix + chr('│')) + '\n' +
    fmt(prefix + chr('│') + '   ') + group + '\n'
}

function renderChildItem (
  childNode: TreeNode,
  last: boolean,
  prefix: string,
  opts: TreeRendererOptions
): string {
  const chr = opts.unicode === false ? asciiChar : unicodeChar
  const more = hasRenderableChildren(childNode.nodes)
  const childConnector = prefix +
    (last ? chr('└') : chr('├')) + chr('─') +
    (more ? chr('┬') : chr('─')) + ' '
  const childPrefix = prefix + (last ? '  ' : chr('│') + ' ')

  return render(opts, {
    node: childNode,
    connector: childConnector,
    prefix: childPrefix,
  })
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

function identity (text: string): string {
  return text
}

function unicodeChar (char: string): string {
  return char
}

function asciiChar (char: string): string {
  const chars: Record<string, string> = {
    '│': '|',
    '└': '`',
    '├': '+',
    '─': '-',
    '┬': '-',
  }
  return chars[char] ?? char
}

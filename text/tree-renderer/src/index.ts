export interface TreeNode {
  label: string
  nodes?: Array<TreeNode | string>
}

export interface TreeRendererOptions {
  /**
   * Formatter applied to tree-drawing character sequences (e.g. `├─┬ `, `│ `).
   * Useful for dimming tree lines so labels stand out: `{ treeChars: chalk.dim }`.
   */
  treeChars?: (chars: string) => string
  /**
   * When false, use ASCII characters (`+`, `` ` ``, `|`, `-`) instead of
   * unicode box-drawing characters. Defaults to true (unicode).
   */
  unicode?: boolean
}

export function renderTree (node: TreeNode | string, opts?: TreeRendererOptions): string {
  return render(node, '', '', opts ?? {})
}

/**
 * @param connector - The formatted connector string for this node's first line
 *   (e.g. `├─┬ `). Empty string for the root node.
 * @param prefix - The raw prefix for subsequent lines and children of this node.
 *   Built from unformatted characters so it can be extended for deeper levels.
 */
function render (
  node: TreeNode | string,
  connector: string,
  prefix: string,
  opts: TreeRendererOptions
): string {
  if (typeof node === 'string') node = { label: node }

  const fmt = opts.treeChars ?? identity
  const chr = opts.unicode === false ? asciiChar : unicodeChar
  const nodes = node.nodes ?? []
  const lines = (node.label || '').split('\n')

  // First line: connector + label
  let result = (connector ? fmt(connector) : '') + lines[0] + '\n'

  // Continuation lines for multiline labels
  const continuationChars = nodes.length ? chr('│') + ' ' : '  '
  for (let l = 1; l < lines.length; l++) {
    result += fmt(prefix + continuationChars) + lines[l] + '\n'
  }

  // Children
  for (let i = 0; i < nodes.length; i++) {
    const child = nodes[i]
    const last = i === nodes.length - 1
    const childNode = typeof child === 'string' ? { label: child } : child
    const more = childNode.nodes != null && childNode.nodes.length > 0

    const childConnector = prefix +
      (last ? chr('└') : chr('├')) + chr('─') +
      (more ? chr('┬') : chr('─')) + ' '
    const childPrefix = prefix + (last ? '  ' : chr('│') + ' ')

    result += render(childNode, childConnector, childPrefix, opts)
  }

  return result
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

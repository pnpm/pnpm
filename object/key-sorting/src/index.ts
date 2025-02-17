import { lexCompare } from '@pnpm/util.lex-comparator'
import _sortKeys from 'sort-keys'

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function sortDirectKeys<T extends { [key: string]: any }> (
  obj: T
): T {
  return _sortKeys<T>(obj, {
    compare: lexCompare,
    deep: false,
  })
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function sortDeepKeys<T extends { [key: string]: any }> (
  obj: T
): T {
  return _sortKeys<T>(obj, {
    compare: lexCompare,
    deep: true,
  })
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function sortKeysByPriority<T extends { [key: string]: any }> (
  opts: {
    priority: Record<string, number>
    deep?: boolean
  },
  obj: T
): T {
  const compare = compareWithPriority.bind(null, opts.priority)
  return _sortKeys(obj, {
    compare,
    deep: opts.deep,
  })
}

function compareWithPriority (priority: Record<string, number>, left: string, right: string): number {
  const leftPriority = priority[left]
  const rightPriority = priority[right]
  if (leftPriority != null && rightPriority != null) return leftPriority - rightPriority
  if (leftPriority != null) return -1
  if (rightPriority != null) return 1
  return lexCompare(left, right)
}

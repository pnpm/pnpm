import { lexCompare } from '@pnpm/text.ordinal-comparator'
import _sortKeys from 'sort-keys'

// eslint-disable-next-line @typescript-eslint/no-explicit-any -- an interface such as a manifest satisfies an index signature of any, not of unknown
export function sortDirectKeys<Obj extends { [key: string]: any }> (
  obj: Obj
): Obj {
  return _sortKeys<Obj>(obj, {
    compare: lexCompare,
    deep: false,
  })
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any -- an interface such as a manifest satisfies an index signature of any, not of unknown
export function sortDeepKeys<Obj extends { [key: string]: any }> (
  obj: Obj
): Obj {
  return _sortKeys<Obj>(obj, {
    compare: lexCompare,
    deep: true,
  })
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any -- an interface such as a manifest satisfies an index signature of any, not of unknown
export function sortKeysByPriority<Obj extends { [key: string]: any }> (
  opts: {
    priority: Record<string, number>
    deep?: boolean
  },
  obj: Obj
): Obj {
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

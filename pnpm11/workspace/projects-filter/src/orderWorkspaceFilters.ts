import type { WorkspaceFilter } from './index.js'

export interface OrderWorkspaceFiltersOptions {
  filter?: string[]
  filterProd?: string[]
  cliOptions?: Record<string, unknown>
  cookedArgv?: string[]
}

/**
 * Combines `filter` and `filterProd` into a single evaluation-ordered list of
 * {@link WorkspaceFilter}s.
 *
 * When both came from CLI flags and `cookedArgv` records their exact relative
 * positions, that interleaved command-line order is preserved. Otherwise
 * `filter` precedes `filterProd`, except when `filter` starts with an
 * exclusion (`!`) while `filterProd` starts with an inclusion, where placing
 * `filterProd` first keeps the inclusion as the base set rather than seeding
 * every workspace project from the leading exclusion.
 */
export function orderWorkspaceFilters (opts: OrderWorkspaceFiltersOptions): WorkspaceFilter[] {
  const filter = opts.filter ?? []
  const filterProd = opts.filterProd ?? []
  if (filter.length === 0) {
    return filterProd.map((selector) => ({ filter: selector, followProdDepsOnly: true }))
  }
  if (filterProd.length === 0) {
    return filter.map((selector) => ({ filter: selector, followProdDepsOnly: false }))
  }
  const interleaved = interleaveCliFilters({
    filter,
    filterProd,
    cliOptions: opts.cliOptions,
    cookedArgv: opts.cookedArgv,
  })
  if (interleaved != null) {
    return interleaved
  }
  const prodFirst = filter[0].startsWith('!') && !filterProd[0].startsWith('!')
  const regularEntries = filter.map((selector) => ({ filter: selector, followProdDepsOnly: false }))
  const prodEntries = filterProd.map((selector) => ({ filter: selector, followProdDepsOnly: true }))
  return prodFirst ? [...prodEntries, ...regularEntries] : [...regularEntries, ...prodEntries]
}

interface CliFilterCursor {
  tokens: string[]
  items: string[]
  splitSingleToken: boolean
  followProdDepsOnly: boolean
  tokenIndex: number
  itemIndex: number
}

function interleaveCliFilters (opts: {
  filter: string[]
  filterProd: string[]
  cliOptions?: Record<string, unknown>
  cookedArgv?: string[]
}): WorkspaceFilter[] | undefined {
  const { cookedArgv, cliOptions, filter, filterProd } = opts
  if (cookedArgv == null || cliOptions == null) return undefined
  const regCursor = createCliFilterCursor(cliOptions['filter'], filter, false)
  const prodCursor = createCliFilterCursor(cliOptions['filter-prod'], filterProd, true)
  if (regCursor == null || prodCursor == null) return undefined

  const ordered = collectInterleavedEntries(cookedArgv, regCursor, prodCursor)
  const allConsumed = regCursor.itemIndex === filter.length && prodCursor.itemIndex === filterProd.length
  return allConsumed ? ordered : undefined
}

interface MatchedFilterToken {
  followProdDepsOnly: boolean
  value: string | undefined
  consumesNext: boolean
}

function collectInterleavedEntries (
  cookedArgv: string[],
  regCursor: CliFilterCursor,
  prodCursor: CliFilterCursor
): WorkspaceFilter[] {
  const ordered: WorkspaceFilter[] = []
  for (let argIndex = 0; argIndex < cookedArgv.length; argIndex++) {
    const token = cookedArgv[argIndex]
    if (token === '--') break
    const matched = matchCliFilterToken(token, cookedArgv[argIndex + 1])
    if (matched == null) continue
    const cursor = matched.followProdDepsOnly ? prodCursor : regCursor
    if (consumeCursor(cursor, matched.value, ordered) && matched.consumesNext) {
      argIndex++
    }
  }
  return ordered
}

function matchCliFilterToken (token: string, nextToken: string | undefined): MatchedFilterToken | undefined {
  if (token === '--filter' || token === '-F') {
    return { followProdDepsOnly: false, value: nextToken, consumesNext: true }
  }
  if (token.startsWith('--filter=')) {
    return { followProdDepsOnly: false, value: token.slice('--filter='.length), consumesNext: false }
  }
  if (isFilterProdFlag(token)) {
    return { followProdDepsOnly: true, value: nextToken, consumesNext: true }
  }
  if (token.startsWith('--filter-')) {
    const eqIndex = token.indexOf('=')
    if (eqIndex !== -1 && isFilterProdFlag(token.slice(0, eqIndex))) {
      return { followProdDepsOnly: true, value: token.slice(eqIndex + 1), consumesNext: false }
    }
  }
  return undefined
}

function isFilterProdFlag (flag: string): boolean {
  return flag.startsWith('--filter-') && '--filter-prod'.startsWith(flag)
}

function consumeCursor (cursor: CliFilterCursor, nextToken: string | undefined, target: WorkspaceFilter[]): boolean {
  if (cursor.tokenIndex >= cursor.tokens.length || nextToken !== cursor.tokens[cursor.tokenIndex]) {
    return false
  }
  cursor.tokenIndex++
  const endIndex = cursor.splitSingleToken ? cursor.items.length : cursor.itemIndex + 1
  while (cursor.itemIndex < endIndex) {
    target.push({ filter: cursor.items[cursor.itemIndex], followProdDepsOnly: cursor.followProdDepsOnly })
    cursor.itemIndex++
  }
  return true
}

function createCliFilterCursor (
  rawValue: unknown,
  items: string[],
  followProdDepsOnly: boolean
): CliFilterCursor | undefined {
  if (Array.isArray(rawValue) && rawValue.every((item): item is string => typeof item === 'string')) {
    if (!arraysEqual(rawValue, items)) return undefined
    return { tokens: rawValue, items, splitSingleToken: false, followProdDepsOnly, tokenIndex: 0, itemIndex: 0 }
  }
  if (typeof rawValue === 'string') {
    if (!arraysEqual(rawValue.split(' '), items)) return undefined
    return { tokens: [rawValue], items, splitSingleToken: true, followProdDepsOnly, tokenIndex: 0, itemIndex: 0 }
  }
  return undefined
}

function arraysEqual (left: string[], right: string[]): boolean {
  return left.length === right.length && left.every((item, index) => item === right[index])
}

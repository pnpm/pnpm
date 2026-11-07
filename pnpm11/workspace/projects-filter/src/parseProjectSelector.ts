import path from 'node:path'

export interface ProjectSelector {
  diff?: string
  exclude?: boolean
  excludeSelf?: boolean
  includeDependencies?: boolean
  includeDependents?: boolean
  namePattern?: string
  parentDir?: string
  followProdDepsOnly?: boolean
  /**
   * Overrides how {@link parentDir} matches, for a selector pnpm generates
   * rather than the user writing it. Left out — every parsed selector —
   * the selector follows the mode the whole filter pass runs in, which
   * `legacyDirFiltering` chooses.
   */
  useGlobDirFiltering?: boolean
}

export function parseProjectSelector (rawSelector: string, prefix: string): ProjectSelector {
  const { exclude, excludeSelf, includeDependencies, includeDependents, selector } = parseSelectorModifiers(rawSelector)
  const matches = selector.match(/^([^.][^{}[\]]*)?(\{[^}]+\})?(\[[^\]]+\])?$/)
  if (matches === null) {
    if (isSelectorByLocation(selector)) {
      return {
        exclude,
        excludeSelf: false,
        parentDir: path.join(prefix, selector),
      }
    }
    return {
      excludeSelf: false,
      namePattern: selector,
    }
  }

  return {
    diff: matches[3]?.slice(1, -1),
    exclude,
    excludeSelf,
    includeDependencies,
    includeDependents,
    namePattern: matches[1],
    parentDir: matches[2] && path.join(prefix, matches[2].slice(1, -1)),
  }
}

interface SelectorModifiers {
  exclude: boolean
  excludeSelf: boolean
  includeDependencies: boolean
  includeDependents: boolean
  selector: string
}

function parseSelectorModifiers (rawSelector: string): SelectorModifiers {
  const exclude = rawSelector[0] === '!'
  let selector = exclude ? rawSelector.substring(1) : rawSelector
  let excludeSelf = false
  const includeDependencies = selector.endsWith('...')
  if (includeDependencies) {
    selector = selector.slice(0, -3)
    if (selector.endsWith('^')) {
      excludeSelf = true
      selector = selector.slice(0, -1)
    }
  }
  const includeDependents = selector.startsWith('...')
  if (includeDependents) {
    selector = selector.substring(3)
    if (selector[0] === '^') {
      excludeSelf = true
      selector = selector.slice(1)
    }
  }
  return { exclude, excludeSelf, includeDependencies, includeDependents, selector }
}

function isSelectorByLocation (rawSelector: string): boolean {
  if (rawSelector[0] !== '.') return false

  // . or ./ or .\
  if (rawSelector.length === 1 || rawSelector[1] === '/' || rawSelector[1] === '\\') return true

  if (rawSelector[1] !== '.') return false

  // .. or ../ or ..\
  return (
    rawSelector.length === 2 ||
    rawSelector[2] === '/' ||
    rawSelector[2] === '\\'
  )
}

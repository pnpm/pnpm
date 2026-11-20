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

const SELECTOR_REGEX = /^([^.][^{}[\]]*)?(\{[^}]+\})?(\[[^\]]+\])?$/

export function parseProjectSelector (rawSelector: string, prefix: string): ProjectSelector {
  const { exclude, excludeSelf, includeDependencies, includeDependents, selector } = parseSelectorModifiers(rawSelector)
  const matches = selector.match(SELECTOR_REGEX)
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

export function rebaseProjectSelector (rawSelector: string, fromPrefix: string, toPrefix: string): string {
  if (path.resolve(fromPrefix) === path.resolve(toPrefix)) {
    return rawSelector
  }
  const { leading, selector, trailing } = parseSelectorModifiers(rawSelector)
  const matches = selector.match(SELECTOR_REGEX)
  if (matches === null) {
    if (isSelectorByLocation(selector)) {
      const rebased = rebaseDir(selector, fromPrefix, toPrefix)
      const location = isSelectorByLocation(rebased) ? rebased : `./${rebased}`
      return `${leading}${location}${trailing}`
    }
    return rawSelector
  }
  if (!matches[2]) {
    return rawSelector
  }
  const rebased = rebaseDir(matches[2].slice(1, -1), fromPrefix, toPrefix)
  return `${leading}${matches[1] ?? ''}{${rebased}}${matches[3] ?? ''}${trailing}`
}

function rebaseDir (rawDir: string, fromPrefix: string, toPrefix: string): string {
  const target = path.join(fromPrefix, rawDir)
  return path.relative(toPrefix, target).replace(/\\/g, '/') || '.'
}

interface SelectorModifiers {
  exclude: boolean
  excludeSelf: boolean
  includeDependencies: boolean
  includeDependents: boolean
  leading: string
  selector: string
  trailing: string
}

function parseSelectorModifiers (rawSelector: string): SelectorModifiers {
  let start = 0
  let end = rawSelector.length
  const exclude = rawSelector[0] === '!'
  if (exclude) {
    start += 1
  }
  let excludeSelf = false
  const includeDependencies = rawSelector.slice(start, end).endsWith('...')
  if (includeDependencies) {
    end -= 3
    if (rawSelector.slice(start, end).endsWith('^')) {
      excludeSelf = true
      end -= 1
    }
  }
  const includeDependents = rawSelector.slice(start, end).startsWith('...')
  if (includeDependents) {
    start += 3
    if (rawSelector.slice(start, end)[0] === '^') {
      excludeSelf = true
      start += 1
    }
  }
  return {
    exclude,
    excludeSelf,
    includeDependencies,
    includeDependents,
    leading: rawSelector.slice(0, start),
    selector: rawSelector.slice(start, end),
    trailing: rawSelector.slice(end),
  }
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

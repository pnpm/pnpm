import path from 'path'

export interface PackageSelector {
  diff?: string
  exclude?: boolean
  excludeSelf?: boolean
  includeDependencies?: boolean
  includeDependents?: boolean
  namePattern?: string
  parentDir?: string
  followProdDepsOnly?: boolean
}

export function parsePackageSelector (rawSelector: string, prefix: string): PackageSelector {
  let exclude = false
  if (rawSelector[0] === '!') {
    exclude = true
    rawSelector = rawSelector.substring(1)
  }
  let excludeSelf = false
  const includeDependencies = rawSelector.endsWith('...')
  if (includeDependencies) {
    rawSelector = rawSelector.slice(0, -3)
    if (rawSelector.endsWith('^')) {
      excludeSelf = true
      rawSelector = rawSelector.slice(0, -1)
    }
  }
  const includeDependents = rawSelector.startsWith('...')
  if (includeDependents) {
    rawSelector = rawSelector.substring(3)
    if (rawSelector.startsWith('^')) {
      excludeSelf = true
      rawSelector = rawSelector.slice(1)
    }
  }
  const matches = rawSelector.match(/^([^.][^{}[\]]*)?(\{[^}]+\})?(\[[^\]]+\])?$/)
  if (matches === null) {
    if (isSelectorByLocation(rawSelector)) {
      return {
        exclude,
        excludeSelf: false,
        parentDir: path.join(prefix, rawSelector),
      }
    }
    return {
      excludeSelf: false,
      namePattern: rawSelector,
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

function isSelectorByLocation (rawSelector: string) {
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

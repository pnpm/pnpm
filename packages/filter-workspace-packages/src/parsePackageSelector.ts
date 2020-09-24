import path = require('path')

export interface PackageSelector {
  diff?: string
  exclude?: boolean
  excludeSelf?: boolean
  includeDependencies?: boolean
  includeDependents?: boolean
  namePattern?: string
  parentDir?: string
}

export default (rawSelector: string, prefix: string): PackageSelector => {
  let exclude = false
  if (rawSelector[0] === '!') {
    exclude = true
    rawSelector = rawSelector.substring(1)
  }
  let excludeSelf = false
  const includeDependencies = rawSelector.endsWith('...')
  if (includeDependencies) {
    rawSelector = rawSelector.substring(0, rawSelector.length - 3)
    if (rawSelector.endsWith('^')) {
      excludeSelf = true
      rawSelector = rawSelector.substr(0, rawSelector.length - 1)
    }
  }
  const includeDependents = rawSelector.startsWith('...')
  if (includeDependents) {
    rawSelector = rawSelector.substring(3)
    if (rawSelector.startsWith('^')) {
      excludeSelf = true
      rawSelector = rawSelector.substr(1)
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
    diff: matches[3]?.substr(1, matches[3].length - 2),
    exclude,
    excludeSelf,
    includeDependencies,
    includeDependents,
    namePattern: matches[1],
    parentDir: matches[2] && path.join(prefix, matches[2].substr(1, matches[2].length - 2)),
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

import path = require('path')

export interface PackageSelector {
  diff?: string,
  excludeSelf?: boolean,
  includeDependencies?: boolean,
  includeDependents?: boolean,
  namePattern?: string,
  parentDir?: string,
}

export default (rawSelector: string, prefix: string): PackageSelector => {
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
  if (rawSelector.startsWith('{') && rawSelector.endsWith('}')) {
    return {
      excludeSelf,
      includeDependencies,
      includeDependents,
      parentDir: path.join(prefix, rawSelector.substr(1, rawSelector.length - 2)),
    }
  }
  if (rawSelector.startsWith('[') && rawSelector.endsWith(']')) {
    return {
      diff: rawSelector.substr(1, rawSelector.length - 2),
      excludeSelf,
      includeDependencies,
      includeDependents,
    }
  }
  if (includeDependencies || includeDependents) {
    return {
      excludeSelf,
      includeDependencies,
      includeDependents,
      namePattern: rawSelector,
    }
  }

  if (isSelectorByLocation(rawSelector)) {
    return {
      excludeSelf: false,
      parentDir: path.join(prefix, rawSelector),
    }
  }
  return {
    excludeSelf: false,
    namePattern: rawSelector,
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

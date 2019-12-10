import path = require('path')

export interface PackageSelector {
  excludeSelf?: boolean,
  pattern: string,
  scope: 'exact' | 'dependencies' | 'dependents',
  selectBy: 'name' | 'location',
}

export default (rawSelector: string, prefix: string): PackageSelector => {
  if (rawSelector.endsWith('^...')) {
    const pattern = rawSelector.substring(0, rawSelector.length - 4)
    return {
      excludeSelf: true,
      pattern,
      scope: 'dependencies',
      selectBy: 'name',
    }
  }
  if (rawSelector.startsWith('...^')) {
    const pattern = rawSelector.substring(4)
    return {
      excludeSelf: true,
      pattern,
      scope: 'dependents',
      selectBy: 'name',
    }
  }
  if (rawSelector.endsWith('...')) {
    const pattern = rawSelector.substring(0, rawSelector.length - 3)
    return {
      excludeSelf: false,
      pattern,
      scope: 'dependencies',
      selectBy: 'name',
    }
  }
  if (rawSelector.startsWith('...')) {
    const pattern = rawSelector.substring(3)
    return {
      excludeSelf: false,
      pattern,
      scope: 'dependents',
      selectBy: 'name',
    }
  }
  if (isSelectorByLocation(rawSelector)) {
    return {
      excludeSelf: false,
      pattern: path.join(prefix, rawSelector),
      scope: 'exact',
      selectBy: 'location',
    }
  }
  return {
    excludeSelf: false,
    pattern: rawSelector,
    scope: 'exact',
    selectBy: 'name',
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

import path = require('path')

export interface PackageSelector {
  pattern: string,
  scope: 'exact' | 'dependencies' | 'dependents',
  selectBy: 'name' | 'location',
}

export default (rawSelector: string, prefix: string): PackageSelector => {
  if (rawSelector.endsWith('...')) {
    const pattern = rawSelector.substring(0, rawSelector.length - 3)
    return {
      pattern,
      scope: 'dependencies',
      selectBy: 'name',
    }
  }
  if (rawSelector.startsWith('...')) {
    const pattern = rawSelector.substring(3)
    return {
      pattern,
      scope: 'dependents',
      selectBy: 'name',
    }
  }
  if (isSelectorByLocation(rawSelector)) {
    return {
      pattern: path.join(prefix, rawSelector),
      scope: 'exact',
      selectBy: 'location',
    }
  }
  return {
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

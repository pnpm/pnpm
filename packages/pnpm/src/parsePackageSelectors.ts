import path = require('path')

export interface PackageSelector {
  matcher: string,
  scope: 'exact' | 'dependencies' | 'dependents',
  selectBy: 'name' | 'location',
}

export default (rawSelector: string, prefix: string): PackageSelector => {
  if (rawSelector.endsWith('...')) {
    const matcher = rawSelector.substring(0, rawSelector.length - 3)
    return {
      matcher,
      scope: 'dependencies',
      selectBy: 'name',
    }
  }
  if (rawSelector.startsWith('...')) {
    const matcher = rawSelector.substring(3)
    return {
      matcher,
      scope: 'dependents',
      selectBy: 'name',
    }
  }
  if (isSelectorByLocation(rawSelector)) {
    return {
      matcher: path.join(prefix, rawSelector),
      scope: 'exact',
      selectBy: 'location',
    }
  }
  return {
    matcher: rawSelector,
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

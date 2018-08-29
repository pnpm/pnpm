export interface PackageSelector {
  matcher: string,
  type: 'exact' | 'dependencies' | 'dependents',
}

export default (rawSelector: string): PackageSelector => {
  if (rawSelector.endsWith('...')) {
    const matcher = rawSelector.substring(0, rawSelector.length - 3)
    return {
      matcher,
      type: 'dependencies',
    }
  }
  if (rawSelector.startsWith('...')) {
    const matcher = rawSelector.substring(3)
    return {
      matcher,
      type: 'dependents',
    }
  }
  return {
    matcher: rawSelector,
    type: 'exact',
  }
}

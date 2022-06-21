export interface NodeEditionSpecifier {
  releaseDir: string
  versionSpecifier: string
}

export function parseNodeEditionSpecifier (specifier: string): NodeEditionSpecifier {
  if (specifier.includes('/')) {
    const [releaseDir, versionSpecifier] = specifier.split('/')
    return { releaseDir, versionSpecifier }
  }
  const prereleaseMatch = specifier.match(/-(nightly|rc|test|v8-canary)/)
  if (prereleaseMatch != null) {
    return { releaseDir: prereleaseMatch[1], versionSpecifier: specifier }
  }
  if (['nightly', 'rc', 'test', 'release', 'v8-canary'].includes(specifier)) {
    return { releaseDir: specifier, versionSpecifier: 'latest' }
  }
  return { releaseDir: 'release', versionSpecifier: specifier }
}

export interface NodeEditionSpecifier {
  releaseChannel: string
  versionSpecifier: string
}

export function parseNodeEditionSpecifier (specifier: string): NodeEditionSpecifier {
  if (specifier.includes('/')) {
    const [releaseChannel, versionSpecifier] = specifier.split('/')
    return { releaseChannel, versionSpecifier }
  }
  const prereleaseMatch = specifier.match(/-(nightly|rc|test|v8-canary)/)
  if (prereleaseMatch != null) {
    return { releaseChannel: prereleaseMatch[1], versionSpecifier: specifier }
  }
  if (['nightly', 'rc', 'test', 'release', 'v8-canary'].includes(specifier)) {
    return { releaseChannel: specifier, versionSpecifier: 'latest' }
  }
  return { releaseChannel: 'release', versionSpecifier: specifier }
}

export function parseNodeVersionSelector (rawVersionSelector: string) {
  if (rawVersionSelector.includes('/')) {
    const [releaseDir, version] = rawVersionSelector.split('/')
    return { releaseDir, version }
  }
  const prereleaseMatch = rawVersionSelector.match(/-(nightly|rc|test|v8-canary)/)
  if (prereleaseMatch != null) {
    return { releaseDir: prereleaseMatch[1], version: rawVersionSelector }
  }
  if (['nightly', 'rc', 'test', 'release', 'v8-canary'].includes(rawVersionSelector)) {
    return { releaseDir: rawVersionSelector, version: 'latest' }
  }
  return { releaseDir: 'release', version: rawVersionSelector }
}

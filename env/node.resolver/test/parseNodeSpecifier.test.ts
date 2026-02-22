import { parseNodeSpecifier } from '../lib/parseNodeSpecifier.js'

test.each([
  // Semver ranges → release channel
  ['6', '6', 'release'],
  ['16.0', '16.0', 'release'],
  // Exact prerelease with rc channel
  ['16.0.0-rc.0', '16.0.0-rc.0', 'rc'],
  // Channel/range combo (major only)
  ['rc/10', '10', 'rc'],
  // Standalone channel name → latest from that channel
  ['nightly', 'latest', 'nightly'],
  ['rc', 'latest', 'rc'],
  ['test', 'latest', 'test'],
  ['v8-canary', 'latest', 'v8-canary'],
  ['release', 'latest', 'release'],
  // Well-known aliases
  ['lts', 'lts', 'release'],
  ['latest', 'latest', 'release'],
  // LTS codenames
  ['argon', 'argon', 'release'],
  ['iron', 'iron', 'release'],
  // Exact stable version
  ['22.0.0', '22.0.0', 'release'],
  // Stable release with explicit channel prefix
  ['release/22.0.0', '22.0.0', 'release'],
  // Channel/version combos
  ['rc/18', '18', 'rc'],
  ['rc/18.0.0-rc.4', '18.0.0-rc.4', 'rc'],
  ['nightly/latest', 'latest', 'nightly'],
  // Exact nightly version
  ['24.0.0-nightly20250315d765e70802', '24.0.0-nightly20250315d765e70802', 'nightly'],
  // Exact v8-canary version
  ['22.0.0-v8-canary20250101abc', '22.0.0-v8-canary20250101abc', 'v8-canary'],
])('Node.js version specifier is parsed: %s', (specifier, expectedVersionSpecifier, expectedReleaseChannel) => {
  const result = parseNodeSpecifier(specifier)
  expect(result.versionSpecifier).toBe(expectedVersionSpecifier)
  expect(result.releaseChannel).toBe(expectedReleaseChannel)
})

test('throws for release channel with invalid version format', () => {
  expect(() => parseNodeSpecifier('release/16.0.0.release.0')).toThrow(
    '"release/16.0.0.release.0" is not a valid Node.js version'
  )
})

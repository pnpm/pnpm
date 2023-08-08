import { parseEnvSpecifier } from '../lib/parseEnvSpecifier'

test.each([
  ['6', '6', 'release'],
  ['16.0.0-rc.0', '16.0.0-rc.0', 'rc'],
  ['rc/10', '10', 'rc'],
  ['nightly', 'latest', 'nightly'],
  ['lts', 'lts', 'release'],
  ['argon', 'argon', 'release'],
  ['latest', 'latest', 'release'],
])('Node.js version selector is parsed', (editionSpecifier, versionSpecifier, releaseChannel) => {
  const node = parseEnvSpecifier(editionSpecifier)
  expect(node.versionSpecifier).toMatch(versionSpecifier)
  expect(node.releaseChannel).toBe(releaseChannel)
})

import { parseNodeVersionSelector } from '../lib/parseNodeVersionSelector'

test.each([
  ['6', '6', 'release'],
  ['16.0.0-rc.0', '16.0.0-rc.0', 'rc'],
  ['rc/10', '10', 'rc'],
  ['nightly', 'latest', 'nightly'],
  ['lts', 'lts', 'release'],
  ['argon', 'argon', 'release'],
  ['latest', 'latest', 'release'],
])('Node.js version selector is parsed', (spec, version, releaseDir) => {
  const node = parseNodeVersionSelector(spec)
  expect(node.version).toMatch(version)
  expect(node.releaseDir).toBe(releaseDir)
})

import { parseNodeEditionSpecifier } from '../lib/parseNodeEditionSpecifier'

test.each([
  ['rc/16.0.0-rc.0', '16.0.0-rc.0', 'rc'],
  ['16.0.0-rc.0', '16.0.0-rc.0', 'rc'],
  ['release/16.0.0', '16.0.0', 'release'],
  ['16.0.0', '16.0.0', 'release'],
])('Node.js version selector is parsed', (editionSpecifier, versionSpecifier, releaseChannel) => {
  const node = parseNodeEditionSpecifier(editionSpecifier)
  expect(node.versionSpecifier).toMatch(versionSpecifier)
  expect(node.releaseChannel).toBe(releaseChannel)
})

test.each([
  ['rc/10', '10', 'rc'],
  ['rc/10.0', '10.0', 'rc'],
  ['rc/10.0.0', '10.0.0', 'rc'],
  ['rc/10.0.0.test.0', '10.0.0.test.0', 'rc'],
])('invalid Node.js specifier', (editionSpecifier, versionSpecifier, releaseChannel) => {
  expect(() => parseNodeEditionSpecifier(editionSpecifier)).toThrow(`The node version (${versionSpecifier}) must contain the release channel (${releaseChannel})`)
})

test.each([
  ['nightly'],
  ['rc'],
  ['test'],
  ['v8-canary'],
])('invalid Node.js specifier', async (specifier) => {
  const promise = Promise.resolve().then(() => parseNodeEditionSpecifier(specifier))
  await expect(promise).rejects.toThrow(`"${specifier}" is not a valid node version`)
  await expect(promise).rejects.toHaveProperty('hint', `The correct syntax for ${specifier} release is strictly X.Y.Z-${specifier}.W`)
})

test.each([
  ['release'],
  ['stable'],
  ['latest'],
  ['release/16.0.0.release.0'],
  ['16'],
  ['16.0'],
])('invalid Node.js specifier', async (specifier) => {
  const promise = Promise.resolve().then(() => parseNodeEditionSpecifier(specifier))
  await expect(promise).rejects.toThrow(`"${specifier}" is not a valid node version`)
  await expect(promise).rejects.toHaveProperty('hint', 'The correct syntax for stable release is strictly X.Y.Z or release/X.Y.Z')
})

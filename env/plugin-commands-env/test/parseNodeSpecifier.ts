import { isValidVersion, parseNodeSpecifier } from '../lib/parseNodeSpecifier'

test.each([
  ['rc/16.0.0-rc.0', '16.0.0-rc.0', 'rc'],
  ['16.0.0-rc.0', '16.0.0-rc.0', 'rc'],
  ['release/16.0.0', '16.0.0', 'release'],
  ['16.0.0', '16.0.0', 'release'],
])('Node.js version selector is parsed', (editionSpecifier, useNodeVersion, releaseChannel) => {
  const node = parseNodeSpecifier(editionSpecifier)
  expect(node.useNodeVersion).toBe(useNodeVersion)
  expect(node.releaseChannel).toBe(releaseChannel)
})

test.each([
  ['rc/10', '10', 'rc'],
  ['rc/10.0', '10.0', 'rc'],
  ['rc/10.0.0', '10.0.0', 'rc'],
  ['rc/10.0.0.test.0', '10.0.0.test.0', 'rc'],
])('invalid Node.js specifier', (editionSpecifier, useNodeVersion, releaseChannel) => {
  expect(() => parseNodeSpecifier(editionSpecifier)).toThrow(`Node.js version (${useNodeVersion}) must contain the release channel (${releaseChannel})`)
})

test.each([
  ['nightly'],
  ['rc'],
  ['test'],
  ['v8-canary'],
])('invalid Node.js specifier', async (specifier) => {
  const promise = Promise.resolve().then(() => parseNodeSpecifier(specifier))
  await expect(promise).rejects.toThrow(`"${specifier}" is not a valid Node.js version`)
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
  const promise = Promise.resolve().then(() => parseNodeSpecifier(specifier))
  await expect(promise).rejects.toThrow(`"${specifier}" is not a valid Node.js version`)
  await expect(promise).rejects.toHaveProperty('hint', 'The correct syntax for stable release is strictly X.Y.Z or release/X.Y.Z')
})

test.each([
  ['rc/16.0.0-rc.0', '16.0.0-rc.0', 'rc'],
  ['16.0.0-rc.0', '16.0.0-rc.0', 'rc'],
  ['release/16.0.0', '16.0.0', 'release'],
  ['16.0.0', '16.0.0', 'release'],
])('valid Node.js specifier', async (specifier) => {
  expect(isValidVersion(specifier)).toBe(true)
})

test.each([
  ['nightly'],
  ['rc'],
  ['test'],
  ['v8-canary'],
  ['release'],
  ['stable'],
  ['latest'],
  ['release/16.0.0.release.0'],
  ['16'],
  ['16.0'],
])('invalid Node.js specifier', async (specifier) => {
  expect(isValidVersion(specifier)).toBe(false)
})

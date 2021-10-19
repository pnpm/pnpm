import resolveNodeVersion from '@pnpm/plugin-commands-env/lib/resolveNodeVersion'

test.each([
  ['6', '6.17.1', 'release'],
  ['16.0.0-rc.0', '16.0.0-rc.0', 'rc'],
  ['rc/10', '10.23.0-rc.0', 'rc'],
  ['nightly', /.+/, 'nightly'],
  ['lts', /.+/, 'release'],
  ['argon', '4.9.1', 'release'],
  ['latest', /.+/, 'release'],
])('Node.js %s is resolved', async (spec, version, releaseDir) => {
  const node = await resolveNodeVersion(spec)
  expect(node.version).toMatch(version)
  expect(node.releaseDir).toBe(releaseDir)
})

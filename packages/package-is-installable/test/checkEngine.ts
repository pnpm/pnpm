import checkEngine from '../lib/checkEngine'

const packageId = 'registry.npmjs.org/foo/1.0.0'

test('no engine defined', () => {
  expect(checkEngine(packageId, {}, { pnpm: '1.1.2', node: '0.2.1' })).toBe(null)
})

test('node version too old', () => {
  const err = checkEngine(packageId, { node: '0.10.24' }, { pnpm: '1.1.2', node: '0.10.18' })
  expect(err).toBeTruthy()
  expect(err?.wanted.node).toBe('0.10.24')
})

test('pnpm version too old', () => {
  const err = checkEngine(packageId, { pnpm: '^1.4.6' }, { pnpm: '1.3.2', node: '0.2.1' })
  expect(err).toBeTruthy()
  expect(err?.wanted.pnpm).toBe('^1.4.6')
})

test('engine is supported', () => {
  expect(checkEngine(packageId, { pnpm: '1', node: '10' }, { pnpm: '1.3.2', node: '10.2.1' })).toBe(null)
})

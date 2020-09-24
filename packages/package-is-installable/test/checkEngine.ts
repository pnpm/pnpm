import checkEngine from '../lib/checkEngine'
import test = require('tape')

const packageId = 'registry.npmjs.org/foo/1.0.0'

test('no engine defined', (t) => {
  t.equals(checkEngine(packageId, {}, { pnpm: '1.1.2', node: '0.2.1' }), null)
  t.end()
})

test('node version too old', (t) => {
  const err = checkEngine(packageId, { node: '0.10.24' }, { pnpm: '1.1.2', node: '0.10.18' })
  t.ok(err, 'returns an error')
  t.equals(err?.wanted.node, '0.10.24')
  t.end()
})

test('pnpm version too old', (t) => {
  const err = checkEngine(packageId, { pnpm: '^1.4.6' }, { pnpm: '1.3.2', node: '0.2.1' })
  t.ok(err, 'returns an error')
  t.equals(err?.wanted.pnpm, '^1.4.6')
  t.end()
})

test('engine is supported', (t) => {
  t.equals(checkEngine(packageId, { pnpm: '1', node: '10' }, { pnpm: '1.3.2', node: '10.2.1' }), null)
  t.end()
})

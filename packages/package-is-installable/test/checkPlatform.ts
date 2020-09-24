import checkPlatform from '../lib/checkPlatform'
import test = require('tape')

const packageId = 'registry.npmjs.org/foo/1.0.0'

test('target cpu wrong', (t) => {
  const target = {
    cpu: 'enten-cpu',
    os: 'any',
  }
  const err = checkPlatform(packageId, target)
  t.ok(err, 'error present')
  t.equal(err.code, 'ERR_PNPM_UNSUPPORTED_PLATFORM')
  t.end()
})

test('os wrong', (t) => {
  const target = {
    cpu: 'any',
    os: 'enten-os',
  }
  const err = checkPlatform(packageId, target)
  t.ok(err, 'error present')
  t.equal(err.code, 'ERR_PNPM_UNSUPPORTED_PLATFORM')
  t.end()
})

test('nothing wrong', (t) => {
  const target = {
    cpu: 'any',
    os: 'any',
  }
  t.notOk(checkPlatform(packageId, target))
  t.end()
})

test('only target cpu wrong', (t) => {
  const err = checkPlatform(packageId, { cpu: 'enten-cpu', os: 'any' })
  t.ok(err, 'error present')
  t.equal(err.code, 'ERR_PNPM_UNSUPPORTED_PLATFORM')
  t.end()
})

test('only os wrong', (t) => {
  const err = checkPlatform(packageId, { cpu: 'any', os: 'enten-os' })
  t.ok(err, 'error present')
  t.equal(err.code, 'ERR_PNPM_UNSUPPORTED_PLATFORM')
  t.end()
})

test('everything wrong w/arrays', (t) => {
  const err = checkPlatform(packageId, { cpu: ['enten-cpu'], os: ['enten-os'] })
  t.ok(err, 'error present')
  t.equal(err.code, 'ERR_PNPM_UNSUPPORTED_PLATFORM')
  t.end()
})

test('os wrong (negation)', (t) => {
  const err = checkPlatform(packageId, { cpu: 'any', os: `!${process.platform}` })
  t.ok(err, 'error present')
  t.equal(err.code, 'ERR_PNPM_UNSUPPORTED_PLATFORM')
  t.end()
})

test('nothing wrong (negation)', (t) => {
  t.deepEqual(checkPlatform(packageId, { cpu: '!enten-cpu', os: '!enten-os' }), null)
  t.end()
})

import matcher from '@pnpm/matcher'
import test = require('tape')

test('matcher()', (t) => {
  {
    const match = matcher('*')
    t.ok(match('@eslint/plugin-foo'))
    t.ok(match('express'))
  }
  {
    const match = matcher(['eslint-*'])
    t.ok(match('eslint-plugin-foo'))
    t.notOk(match('express'))
  }
  {
    const match = matcher(['*plugin*'])
    t.ok(match('@eslint/plugin-foo'))
    t.notOk(match('express'))
  }
  {
    const match = matcher(['a*c'])
    t.ok(match('abc'))
  }
  {
    const match = matcher(['*-positive'])
    t.ok(match('is-positive'))
  }
  {
    const match = matcher(['foo', 'bar'])
    t.ok(match('foo'))
    t.ok(match('bar'))
    t.notOk(match('express'))
  }
  t.end()
})

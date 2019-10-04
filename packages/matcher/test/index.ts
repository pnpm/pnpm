import matcher from '@pnpm/matcher'
import test = require('tape')

test('matcher()', (t) => {
  {
    const match = matcher('eslint-*')
    t.ok(match('eslint-plugin-foo'))
    t.notOk(match('express'))
  }
  {
    const match = matcher('*plugin*')
    t.ok(match('@eslint/plugin-foo'))
    t.notOk(match('express'))
  }
  t.end()
})

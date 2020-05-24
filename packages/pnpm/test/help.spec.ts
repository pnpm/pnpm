import test = require('tape')
import createHelp from '../src/cmd/help'

test('print an error when help not found', (t) => {
  t.equal(
    createHelp({})({}, ['foo']).split('\n')[1],
    'No results for "foo"'
  )
  t.end()
})

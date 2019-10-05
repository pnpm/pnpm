import createPackagesSearcher from '@pnpm/list/lib/createPackagesSearcher'
import test = require('tape')

test('packages searcher', (t) => {
  {
    const search = createPackagesSearcher(['rimraf@*'])
    t.ok(search({ name: 'rimraf', version: '1.0.0' }))
    t.notOk(search({ name: 'express', version: '1.0.0' }))
  }
  {
    const search = createPackagesSearcher(['rim*'])
    t.ok(search({ name: 'rimraf', version: '1.0.0' }))
    t.notOk(search({ name: 'express', version: '1.0.0' }))
  }
  {
    const search = createPackagesSearcher(['rim*@2'])
    t.ok(search({ name: 'rimraf', version: '2.0.0' }))
    t.notOk(search({ name: 'rimraf', version: '1.0.0' }))
  }
  {
    const search = createPackagesSearcher(['minimatch', 'once@1.4'])
    t.ok(search({ name: 'minimatch', version: '2.0.0' }))
    t.ok(search({ name: 'once', version: '1.4.1' }))
    t.notOk(search({ name: 'rimraf', version: '1.0.0' }))
  }
  t.end()
})

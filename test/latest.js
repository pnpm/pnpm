
var test = require('tape')
var latest = require('../lib/latest')

test('@rstacruz/tap-spec', function (t) {
  var pkg = {
    raw: '@rstacruz/tap-spec',
    scope: '@rstacruz',
    name: '@rstacruz/tap-spec',
    rawSpec: '',
    spec: 'latest',
    type: 'tag'
  }

  latest(pkg).then(function (latest) {
    t.equal(latest, '4.1.1')
    t.end()
  })
});

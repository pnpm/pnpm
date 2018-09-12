import test = require('tape')
import {satisfiesPackageJson} from 'pnpm-shrinkwrap'

const DEFAULT_SHR_FIELDS = {
  shrinkwrapVersion: 3,
  registry: 'https://registry.npmjs.org/',
}

const DEFAULT_PKG_FIELDS = {
  name: 'project',
  version: '1.0.0',
}

test('satisfiesPackageJson()', t => {
  t.ok(satisfiesPackageJson({
    ...DEFAULT_SHR_FIELDS,
    dependencies: {foo: '1.0.0'},
    specifiers: {foo: '^1.0.0'},
  }, {
    ...DEFAULT_PKG_FIELDS,
    dependencies: {foo: '^1.0.0'},
  }))
  t.ok(satisfiesPackageJson({
    ...DEFAULT_SHR_FIELDS,
    dependencies: {foo: '1.0.0'},
    devDependencies: {},
    specifiers: {foo: '^1.0.0'},
  }, {
    ...DEFAULT_PKG_FIELDS,
    dependencies: {foo: '^1.0.0'}
  }))
  t.ok(satisfiesPackageJson({
    ...DEFAULT_SHR_FIELDS,
    devDependencies: {foo: '1.0.0'},
    specifiers: {foo: '^1.0.0'},
  }, {
    ...DEFAULT_PKG_FIELDS,
    devDependencies: {foo: '^1.0.0'},
  }))
  t.ok(satisfiesPackageJson({
    ...DEFAULT_SHR_FIELDS,
    optionalDependencies: {foo: '1.0.0'},
    specifiers: {foo: '^1.0.0'},
  }, {
    ...DEFAULT_PKG_FIELDS,
    optionalDependencies: {foo: '^1.0.0'},
  }))
  t.notOk(satisfiesPackageJson({
    ...DEFAULT_SHR_FIELDS,
    dependencies: {foo: '1.0.0'},
    specifiers: {foo: '^1.0.0'},
  }, {
    ...DEFAULT_PKG_FIELDS,
    optionalDependencies: {foo: '^1.0.0'},
  }), 'dep type differs')
  t.notOk(satisfiesPackageJson({
    ...DEFAULT_SHR_FIELDS,
    dependencies: {foo: '1.0.0'},
    specifiers: {foo: '^1.0.0'},
  }, {
    ...DEFAULT_PKG_FIELDS,
    dependencies: {foo: '^1.1.0'},
  }), 'spec does not match' )
  t.notOk(satisfiesPackageJson({
    ...DEFAULT_SHR_FIELDS,
    dependencies: {foo: '1.0.0'},
    specifiers: {foo: '^1.0.0'},
  }, {
    ...DEFAULT_PKG_FIELDS,
    dependencies: {foo: '^1.0.0', bar: '2.0.0'},
  }), 'dep spec missing')
  t.notOk(satisfiesPackageJson({
    ...DEFAULT_SHR_FIELDS,
    dependencies: {foo: '1.0.0'},
    specifiers: {foo: '^1.0.0', bar: '2.0.0'},
  }, {
    ...DEFAULT_PKG_FIELDS,
    dependencies: {foo: '^1.0.0', bar: '2.0.0'},
  }))

  {
    const shr = {
      ...DEFAULT_SHR_FIELDS,
      dependencies: {
        foo: '1.0.0'
      },
      optionalDependencies: {
        bar: '2.0.0'
      },
      specifiers: {
        bar: '2.0.0',
        foo: '^1.0.0'
      }
    }
    const pkg = {
      ...DEFAULT_PKG_FIELDS,
      dependencies: {
        bar: '2.0.0',
        foo: '^1.0.0'
      },
      optionalDependencies: {
        bar: '2.0.0'
      }
    }
    t.ok(satisfiesPackageJson(shr, pkg))
  }

  {
    const shr = {
      ...DEFAULT_SHR_FIELDS,
      dependencies: {
        bar: '2.0.0',
        qar: '1.0.0',
      },
      specifiers: {
        bar: '2.0.0',
        qar: '^1.0.0'
      }
    }
    const pkg = {
      ...DEFAULT_PKG_FIELDS,
      dependencies: {
        bar: '2.0.0'
      },
    }
    t.notOk(satisfiesPackageJson(shr, pkg))
  }

  {
    const shr = {
      ...DEFAULT_SHR_FIELDS,
      dependencies: {
        bar: '2.0.0',
        qar: '1.0.0',
      },
      specifiers: {
        bar: '2.0.0',
      }
    }
    const pkg = {
      ...DEFAULT_PKG_FIELDS,
      dependencies: {
        bar: '2.0.0'
      },
    }
    t.notOk(satisfiesPackageJson(shr, pkg))
  }

  t.ok(satisfiesPackageJson({
    ...DEFAULT_SHR_FIELDS,
    dependencies: {foo: '1.0.0', linked: 'link:../linked'},
    specifiers: {foo: '^1.0.0'},
  }, {
    ...DEFAULT_PKG_FIELDS,
    dependencies: {foo: '^1.0.0'},
  }), 'linked packages that are not in package.json are ignored')

  t.end()
})

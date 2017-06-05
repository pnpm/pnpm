import test = require('tape')
import prune from '../src/prune'

test('remove one redundant package', t => {
  t.deepEqual(prune({
    version: 3,
    registry: 'https://registry.npmjs.org',
    dependencies: {
      'is-positive': '1.0.0'
    },
    specifiers: {
      'is-positive': '^1.0.0'
    },
    packages: {
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-positive/2.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      }
    }
  }, {
    name: 'foo',
    version: '1.0.0',
    dependencies: {
      'is-positive': '^1.0.0'
    }
  }), {
    version: 3,
    registry: 'https://registry.npmjs.org',
    dependencies: {
      'is-positive': '1.0.0'
    },
    specifiers: {
      'is-positive': '^1.0.0'
    },
    packages: {
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      }
    }
  })

  t.end()
})

test('keep all', t => {
  t.deepEqual(prune({
    version: 3,
    registry: 'https://registry.npmjs.org',
    dependencies: {
      'is-positive': '1.0.0',
      'is-negative': '1.0.0',
    },
    specifiers: {
      'is-positive': '^1.0.0',
      'is-negative': '^1.0.0',
    },
    packages: {
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-negative/1.0.0': {
        dependencies: {
          'is-positive': '2.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-positive/2.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
    }
  }, {
    name: 'foo',
    version: '1.0.0',
    dependencies: {
      'is-positive': '^1.0.0',
      'is-negative': '^1.0.0',
    }
  }), {
    version: 3,
    registry: 'https://registry.npmjs.org',
    dependencies: {
      'is-positive': '1.0.0',
      'is-negative': '1.0.0',
    },
    specifiers: {
      'is-positive': '^1.0.0',
      'is-negative': '^1.0.0',
    },
    packages: {
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-negative/1.0.0': {
        dependencies: {
          'is-positive': '2.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/is-positive/2.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
    }
  })

  t.end()
})

test('optional dependency should have optional = true', t => {
  t.deepEqual(prune({
    version: 3,
    registry: 'https://registry.npmjs.org',
    dependencies: {
      'pkg-with-good-optional': '1.0.0',
    },
    optionalDependencies: {
      'is-positive': '1.0.0',
    },
    specifiers: {
      'is-positive': '^1.0.0',
      'pkg-with-good-optional': '^1.0.0',
    },
    packages: {
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/pkg-with-good-optional/1.0.0': {
        optionalDependencies: {
          'is-positive': '1.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
    }
  }, {
    name: 'foo',
    version: '1.0.0',
    optionalDependencies: {
      'is-positive': '^1.0.0',
    },
    dependencies: {
      'pkg-with-good-optional': '^1.0.0',
    },
  }), {
    version: 3,
    registry: 'https://registry.npmjs.org',
    dependencies: {
      'pkg-with-good-optional': '1.0.0',
    },
    optionalDependencies: {
      'is-positive': '1.0.0',
    },
    specifiers: {
      'is-positive': '^1.0.0',
      'pkg-with-good-optional': '^1.0.0',
    },
    packages: {
      '/is-positive/1.0.0': {
        optional: true,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/pkg-with-good-optional/1.0.0': {
        optionalDependencies: {
          'is-positive': '1.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
    }
  })

  t.end()
})

test('optional dependency should not have optional = true if used not only as optional', t => {
  t.deepEqual(prune({
    version: 3,
    registry: 'https://registry.npmjs.org',
    dependencies: {
      'pkg-with-good-optional': '1.0.0',
      'is-positive': '1.0.0',
    },
    specifiers: {
      'is-positive': '^1.0.0',
      'pkg-with-good-optional': '^1.0.0',
    },
    packages: {
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/pkg-with-good-optional/1.0.0': {
        optionalDependencies: {
          'is-positive': '1.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
    }
  }, {
    name: 'foo',
    version: '1.0.0',
    dependencies: {
      'pkg-with-good-optional': '^1.0.0',
      'is-positive': '^1.0.0',
    },
  }), {
    version: 3,
    registry: 'https://registry.npmjs.org',
    dependencies: {
      'pkg-with-good-optional': '1.0.0',
      'is-positive': '1.0.0',
    },
    specifiers: {
      'is-positive': '^1.0.0',
      'pkg-with-good-optional': '^1.0.0',
    },
    packages: {
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/pkg-with-good-optional/1.0.0': {
        optionalDependencies: {
          'is-positive': '1.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
    }
  })

  t.end()
})

test('dev dependency should have dev = true', t => {
  t.deepEqual(prune({
    version: 3,
    registry: 'https://registry.npmjs.org',
    dependencies: {
      'pkg-with-good-optional': '1.0.0',
    },
    devDependencies: {
      'is-positive': '1.0.0',
    },
    specifiers: {
      'is-positive': '^1.0.0',
      'pkg-with-good-optional': '^1.0.0',
    },
    packages: {
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/pkg-with-good-optional/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
    }
  }, {
    name: 'foo',
    version: '1.0.0',
    devDependencies: {
      'is-positive': '^1.0.0',
    },
    dependencies: {
      'pkg-with-good-optional': '^1.0.0',
    },
  }), {
    version: 3,
    registry: 'https://registry.npmjs.org',
    dependencies: {
      'pkg-with-good-optional': '1.0.0',
    },
    devDependencies: {
      'is-positive': '1.0.0',
    },
    specifiers: {
      'is-positive': '^1.0.0',
      'pkg-with-good-optional': '^1.0.0',
    },
    packages: {
      '/is-positive/1.0.0': {
        dev: true,
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/pkg-with-good-optional/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
    }
  })

  t.end()
})

test('dev dependency should not have dev = true if it is used not only as dev', t => {
  t.deepEqual(prune({
    version: 3,
    registry: 'https://registry.npmjs.org',
    dependencies: {
      'some-pkg': '1.0.0',
    },
    devDependencies: {
      'is-positive': '1.0.0',
    },
    specifiers: {
      'is-positive': '^1.0.0',
      'some-pkg': '^1.0.0',
    },
    packages: {
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/some-pkg/1.0.0': {
        dependencies: {
          'is-positive': '1.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
    }
  }, {
    name: 'foo',
    version: '1.0.0',
    devDependencies: {
      'is-positive': '^1.0.0',
    },
    dependencies: {
      'some-pkg': '^1.0.0',
    },
  }), {
    version: 3,
    registry: 'https://registry.npmjs.org',
    dependencies: {
      'some-pkg': '1.0.0',
    },
    devDependencies: {
      'is-positive': '1.0.0',
    },
    specifiers: {
      'is-positive': '^1.0.0',
      'some-pkg': '^1.0.0',
    },
    packages: {
      '/is-positive/1.0.0': {
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
      '/some-pkg/1.0.0': {
        dependencies: {
          'is-positive': '1.0.0',
        },
        resolution: {
          integrity: 'sha1-ChbBDewTLAqLCzb793Fo5VDvg/g='
        }
      },
    }
  })

  t.end()
})

import { ResolveFunction } from '@pnpm/default-resolver'
import { getLatestManifest } from 'pnpm/src/createLatestManifestGetter'
import tape = require('tape')
import promisifyTape from 'tape-promise'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('getLatestManifest()', async (t: tape.Test) => {
  t.plan(4)

  const opts = {
    lockfileDir: '',
    registries: {
      '@scope': 'https://pnpm.js.org/',
      'default': 'https://registry.npmjs.org/',
    },
    workingDir: '',
  }
  {
    const resolve: ResolveFunction = async function (wantedPackage, opts) {
      t.equal(opts.registry, 'https://registry.npmjs.org/')
      return {
        id: 'foo/1.0.0',
        latest: '1.0.0',
        package: {
          name: 'foo',
          version: '1.0.0',
        },
        resolution: {
          type: 'tarball',
        },
        resolvedVia: 'npm-registry'
      }
    }
    t.deepEqual(await getLatestManifest(resolve, opts, 'foo'), {
      name: 'foo',
      version: '1.0.0',
    })
  }
  {
    const resolve: ResolveFunction = async function (wantedPackage, opts) {
      t.equal(opts.registry, 'https://pnpm.js.org/')
      return {
        id: 'foo/2.0.0',
        latest: '2.0.0',
        package: {
          name: 'foo',
          version: '2.0.0',
        },
        resolution: {
          type: 'tarball',
        },
        resolvedVia: 'npm-registry'
      }
    }
    t.deepEqual(await getLatestManifest(resolve, opts, '@scope/foo'), {
      name: 'foo',
      version: '2.0.0',
    })
  }
})

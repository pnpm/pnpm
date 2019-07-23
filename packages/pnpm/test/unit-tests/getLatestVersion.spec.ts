import { ResolveFunction } from '@pnpm/default-resolver'
import { getLatestVersion } from 'pnpm/src/createLatestVersionGetter'
import tape = require('tape')
import promisifyTape from 'tape-promise'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('getLatestVersion()', async (t: tape.Test) => {
  t.plan(4)

  const opts = {
    lockfileDirectory: '',
    prefix: '',
    registries: {
      '@scope': 'https://pnpm.js.org/',
      'default': 'https://registry.npmjs.org/',
    },
  }
  {
    const resolve: ResolveFunction = async function (wantedPackage, opts) {
      t.equal(opts.registry, 'https://registry.npmjs.org/')
      return {
        id: 'foo/1.0.0',
        latest: '1.0.0',
        resolution: {
          type: 'tarball',
        },
        resolvedVia: 'npm-registry'
      }
    }
    t.equal(await getLatestVersion(resolve, opts, 'foo'), '1.0.0')
  }
  {
    const resolve: ResolveFunction = async function (wantedPackage, opts) {
      t.equal(opts.registry, 'https://pnpm.js.org/')
      return {
        id: 'foo/2.0.0',
        latest: '2.0.0',
        resolution: {
          type: 'tarball',
        },
        resolvedVia: 'npm-registry'
      }
    }
    t.equal(await getLatestVersion(resolve, opts, '@scope/foo'), '2.0.0')
  }
})

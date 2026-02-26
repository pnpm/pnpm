import { getPkgInfo, type GetPkgInfoOpts } from '../src/getPkgInfo.js'
import path from 'path'

test('getPkgInfo handles missing pkgSnapshot without crashing', () => {
  const opts: GetPkgInfoOpts = {
    alias: 'missing-pkg',
    ref: '/missing-pkg@1.0.0',
    currentPackages: {}, // empty node_modules
    wantedPackages: {}, // missing from lockfile
    depTypes: {},
    skipped: new Set<string>(),
    registries: {
      default: 'https://registry.npmjs.org/',
    },
    virtualStoreDirMaxLength: 120,
    modulesDir: '',
    linkedPathBaseDir: '',
  }

  const result = getPkgInfo(opts)

  expect(result.pkgInfo).toEqual({
    alias: 'missing-pkg',
    name: 'missing-pkg',
    version: '/missing-pkg@1.0.0',
    isMissing: true,
    isPeer: false,
    isSkipped: false,
    path: path.join('.pnpm/missing-pkg@1.0.0/node_modules/missing-pkg'),
  })
})
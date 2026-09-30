import path from 'node:path'

import { expect, test } from '@jest/globals'

import { getPkgInfo, type GetPkgInfoOpts } from '../src/getPkgInfo.js'

test('getPkgInfo handles missing pkgSnapshot without crashing', () => {
  const opts: GetPkgInfoOpts = {
    alias: 'missing-pkg',
    ref: 'missing-pkg@1.0.0',
    currentPackages: {},
    wantedPackages: {},
    depTypes: {},
    skipped: new Set<string>(),
    registriesByScope: {
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
    version: 'missing-pkg@1.0.0',
    isMissing: true,
    isPeer: false,
    isSkipped: false,
    path: path.join('.pnpm/missing-pkg@1.0.0/node_modules/missing-pkg'),
  })
  expect(result.pkgInfo.resolved).toBeUndefined()
  expect(result.pkgInfo.optional).toBeUndefined()
})

test('resolvePackagePath returns virtualStoreDir for unsafe package name', async () => {
  const { resolvePackagePath } = await import('../src/resolvePackagePath.js')
  const virtualStoreDir = path.resolve('node_modules/.pnpm')

  const isolatedPath = resolvePackagePath({
    depPath: 'pkg@1.0.0',
    name: '../../../../escape',
    alias: 'alias',
    virtualStoreDir,
    virtualStoreDirMaxLength: 120,
    nodeLinker: 'isolated',
  })
  expect(isolatedPath).toBe(virtualStoreDir)

  const hoistedPath = resolvePackagePath({
    depPath: 'pkg@1.0.0',
    name: '../../../../escape',
    alias: 'alias',
    virtualStoreDir,
    virtualStoreDirMaxLength: 120,
    nodeLinker: 'hoisted',
  })
  expect(hoistedPath).toBe(virtualStoreDir)
})

import fs from 'node:fs'
import os from 'node:os'
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

test('resolvePackagePath probes the alias of a hoisted dependency without hoistedLocations', async () => {
  const { resolvePackagePath } = await import('../src/resolvePackagePath.js')
  const lockfileDir = fs.mkdtempSync(path.join(os.tmpdir(), 'tree-builder-alias-'))
  try {
    const aliasedDir = path.join(lockfileDir, 'node_modules', 'bar')
    fs.mkdirSync(aliasedDir, { recursive: true })
    fs.writeFileSync(path.join(aliasedDir, 'package.json'), JSON.stringify({ name: 'foo', version: '1.0.0' }))

    expect(resolvePackagePath({
      depPath: 'foo@1.0.0',
      name: 'foo',
      alias: 'bar',
      version: '1.0.0',
      virtualStoreDir: path.join(lockfileDir, 'node_modules/.pnpm'),
      virtualStoreDirMaxLength: 120,
      nodeLinker: 'hoisted',
      lockfileDir,
    })).toBe(aliasedDir)
  } finally {
    fs.rmSync(lockfileDir, { recursive: true, force: true })
  }
})

test('resolvePackagePath keeps every segment of a nested modulesDir for a hoisted dependency', async () => {
  const { resolvePackagePath } = await import('../src/resolvePackagePath.js')
  const lockfileDir = fs.mkdtempSync(path.join(os.tmpdir(), 'tree-builder-modules-dir-'))
  try {
    const projectDir = path.join(lockfileDir, 'packages/a')
    const pkgDir = path.join(projectDir, 'www/modules/foo')
    fs.mkdirSync(pkgDir, { recursive: true })
    fs.writeFileSync(path.join(pkgDir, 'package.json'), JSON.stringify({ name: 'foo', version: '1.0.0' }))

    expect(resolvePackagePath({
      depPath: 'foo@1.0.0',
      name: 'foo',
      alias: 'foo',
      version: '1.0.0',
      virtualStoreDir: path.join(lockfileDir, 'www/modules/.pnpm'),
      virtualStoreDirMaxLength: 120,
      modulesDir: path.join(lockfileDir, 'www/modules'),
      nodeLinker: 'hoisted',
      lockfileDir,
      projectDir,
    })).toBe(pkgDir)
  } finally {
    fs.rmSync(lockfileDir, { recursive: true, force: true })
  }
})

test('resolvePackagePath picks the hoisted copy that Node.js resolves from the parent', async () => {
  const { resolvePackagePath } = await import('../src/resolvePackagePath.js')
  const lockfileDir = fs.mkdtempSync(path.join(os.tmpdir(), 'tree-builder-closest-'))
  try {
    for (const dir of ['node_modules/foo', 'packages/a/node_modules/foo', 'packages/a/node_modules/bar']) {
      fs.mkdirSync(path.join(lockfileDir, dir), { recursive: true })
    }

    expect(resolvePackagePath({
      depPath: 'foo@1.0.0',
      name: 'foo',
      alias: 'foo',
      version: '1.0.0',
      virtualStoreDir: path.join(lockfileDir, 'node_modules/.pnpm'),
      virtualStoreDirMaxLength: 120,
      modulesDir: path.join(lockfileDir, 'node_modules'),
      nodeLinker: 'hoisted',
      hoistedLocations: { 'foo@1.0.0': ['node_modules/foo', 'packages/a/node_modules/foo'] },
      lockfileDir,
      projectDir: lockfileDir,
      parentDir: path.join(lockfileDir, 'packages/a/node_modules/bar'),
    })).toBe(path.join(lockfileDir, 'packages/a/node_modules/foo'))
  } finally {
    fs.rmSync(lockfileDir, { recursive: true, force: true })
  }
})

test('resolvePackagePath picks the hoisted copy installed under the edge alias', async () => {
  const { resolvePackagePath } = await import('../src/resolvePackagePath.js')
  const lockfileDir = fs.mkdtempSync(path.join(os.tmpdir(), 'tree-builder-aliases-'))
  try {
    for (const dir of ['node_modules/foo', 'node_modules/bar']) {
      fs.mkdirSync(path.join(lockfileDir, dir), { recursive: true })
    }
    const opts = {
      depPath: 'foo@1.0.0',
      name: 'foo',
      version: '1.0.0',
      virtualStoreDir: path.join(lockfileDir, 'node_modules/.pnpm'),
      virtualStoreDirMaxLength: 120,
      modulesDir: path.join(lockfileDir, 'node_modules'),
      nodeLinker: 'hoisted' as const,
      hoistedLocations: { 'foo@1.0.0': ['node_modules/foo', 'node_modules/bar'] },
      lockfileDir,
      projectDir: lockfileDir,
    }

    expect(resolvePackagePath({ ...opts, alias: 'foo' })).toBe(path.join(lockfileDir, 'node_modules/foo'))
    expect(resolvePackagePath({ ...opts, alias: 'bar' })).toBe(path.join(lockfileDir, 'node_modules/bar'))
  } finally {
    fs.rmSync(lockfileDir, { recursive: true, force: true })
  }
})

test('resolvePackagePath does not take a scoped hoisted copy for an unscoped alias', async () => {
  const { resolvePackagePath } = await import('../src/resolvePackagePath.js')
  const lockfileDir = fs.mkdtempSync(path.join(os.tmpdir(), 'tree-builder-scoped-alias-'))
  try {
    for (const dir of ['node_modules/@scope/foo', 'node_modules/foo']) {
      fs.mkdirSync(path.join(lockfileDir, dir), { recursive: true })
    }
    const opts = {
      depPath: 'foo@1.0.0',
      name: 'foo',
      version: '1.0.0',
      virtualStoreDir: path.join(lockfileDir, 'node_modules/.pnpm'),
      virtualStoreDirMaxLength: 120,
      modulesDir: path.join(lockfileDir, 'node_modules'),
      nodeLinker: 'hoisted' as const,
      hoistedLocations: { 'foo@1.0.0': ['node_modules/@scope/foo', 'node_modules/foo'] },
      lockfileDir,
      projectDir: lockfileDir,
    }

    expect(resolvePackagePath({ ...opts, alias: 'foo' })).toBe(path.join(lockfileDir, 'node_modules/foo'))
    expect(resolvePackagePath({ ...opts, alias: '@scope/foo' })).toBe(path.join(lockfileDir, 'node_modules/@scope/foo'))
  } finally {
    fs.rmSync(lockfileDir, { recursive: true, force: true })
  }
})

test('resolvePackagePath prefers a hoisted copy on disk over a missing one under the alias', async () => {
  const { resolvePackagePath } = await import('../src/resolvePackagePath.js')
  const lockfileDir = fs.mkdtempSync(path.join(os.tmpdir(), 'tree-builder-missing-alias-'))
  try {
    fs.mkdirSync(path.join(lockfileDir, 'node_modules/bar'), { recursive: true })

    expect(resolvePackagePath({
      depPath: 'foo@1.0.0',
      name: 'foo',
      alias: 'foo',
      version: '1.0.0',
      virtualStoreDir: path.join(lockfileDir, 'node_modules/.pnpm'),
      virtualStoreDirMaxLength: 120,
      modulesDir: path.join(lockfileDir, 'node_modules'),
      nodeLinker: 'hoisted',
      hoistedLocations: { 'foo@1.0.0': ['node_modules/foo', 'node_modules/bar'] },
      lockfileDir,
      projectDir: lockfileDir,
    })).toBe(path.join(lockfileDir, 'node_modules/bar'))
  } finally {
    fs.rmSync(lockfileDir, { recursive: true, force: true })
  }
})

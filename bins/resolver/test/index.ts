import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { getBinsFromPackageManifest } from '@pnpm/bins.resolver'

test('getBinsFromPackageManifest()', async () => {
  expect(
    await getBinsFromPackageManifest({
      bin: 'one-bin',
      name: 'one-bin',
      version: '1.0.0',
    }, process.cwd())).toStrictEqual(
    [{
      name: 'one-bin',
      path: path.resolve('one-bin'),
    }]
  )
})

test('getBinsFromPackageManifest() should allow $ as command name', async () => {
  expect(
    await getBinsFromPackageManifest({
      bin: {
        $: './undollar.js',
      },
      name: 'undollar',
      version: '1.0.0',
    }, process.cwd())).toStrictEqual(
    [{
      name: '$',
      path: path.resolve('undollar.js'),
    }]
  )
})

test('find all the bin files from a bin directory', async () => {
  const fixtures = path.join(import.meta.dirname, 'fixtures')
  expect(
    await getBinsFromPackageManifest({
      name: 'bin-dir',
      version: '1.0.0',

      directories: { bin: 'bin-dir' },
    }, fixtures)).toStrictEqual(
    [
      {
        name: 'rootBin.js',
        path: path.join(fixtures, 'bin-dir/rootBin.js'),
      },
      {
        name: 'subBin.js',
        path: path.join(fixtures, 'bin-dir/subdir/subBin.js'),
      },
    ]
  )
})

test('get bin of scoped package', async () => {
  expect(
    await getBinsFromPackageManifest({
      bin: 'bin.js',
      name: '@foo/bar',
      version: '1.0.0',
    }, process.cwd())).toStrictEqual(
    [{
      name: 'bar',
      path: path.resolve('bin.js'),
    }]
  )
})

test('skip dangerous bin names', async () => {
  expect(
    await getBinsFromPackageManifest({
      name: 'foo',
      version: '1.0.0',

      bin: {
        '../bad': './bad',
        '..\\bad': './bad',
        good: './good',
        '~/bad': './bad',
      },
    }, process.cwd())).toStrictEqual(
    [
      {
        name: 'good',
        path: path.resolve('good'),
      },
    ]
  )
})

test('skip dangerous bin locations', async () => {
  expect(
    await getBinsFromPackageManifest({
      name: 'foo',
      version: '1.0.0',

      bin: {
        bad: '../bad',
        good: './good',
      },
    }, process.cwd())).toStrictEqual(
    [
      {
        name: 'good',
        path: path.resolve('good'),
      },
    ]
  )
})

test('resolve bin paths that point into node_modules using package resolution', async () => {
  const projectDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-bins-resolver-'))
  try {
    const virtualStoreDir = path.join(projectDir, '.pnpm', 'meta-tool@1.0.0', 'node_modules')
    const realPkgDir = path.join(virtualStoreDir, 'meta-tool')
    const pkgDir = path.join(projectDir, 'node_modules', 'meta-tool')
    const cliDir = path.join(virtualStoreDir, '@scope', 'cli')
    fs.mkdirSync(path.join(cliDir, 'dist'), { recursive: true })
    fs.mkdirSync(realPkgDir, { recursive: true })
    fs.mkdirSync(path.dirname(pkgDir), { recursive: true })
    fs.symlinkSync(realPkgDir, pkgDir, process.platform === 'win32' ? 'junction' : 'dir')
    fs.writeFileSync(path.join(cliDir, 'package.json'), JSON.stringify({
      name: '@scope/cli',
      version: '1.0.0',
    }))
    fs.writeFileSync(path.join(cliDir, 'dist', 'cli.js'), '#!/usr/bin/env node\nconsole.log("ok")\n')

    expect(
      await getBinsFromPackageManifest({
        name: 'meta-tool',
        version: '1.0.0',
        bin: {
          'meta-tool': 'node_modules/@scope/cli/dist/cli.js',
        },
      }, pkgDir)
    ).toStrictEqual([
      {
        name: 'meta-tool',
        path: path.join(cliDir, 'dist', 'cli.js'),
      },
    ])
  } finally {
    fs.rmSync(projectDir, { force: true, recursive: true })
  }
})

test('get bin from scoped bin name', async () => {
  expect(
    await getBinsFromPackageManifest({
      name: '@foo/a',
      version: '1.0.0',
      bin: {
        '@foo/a': './a',
      },
    }, process.cwd())).toStrictEqual(
    [
      {
        name: 'a',
        path: path.resolve('a'),
      },
    ]
  )
})

test('skip scoped bin names with path traversal', async () => {
  expect(
    await getBinsFromPackageManifest({
      name: 'malicious',
      version: '1.0.0',
      bin: {
        '@scope/../../.npmrc': './malicious.js',
        '@scope/../etc/passwd': './evil.js',
        '@scope/legit': './good.js',
      },
    }, process.cwd())).toStrictEqual([
    {
      name: 'legit',
      path: path.resolve('good.js'),
    },
  ])
})

test('skip directories.bin with path traversal', async () => {
  // Security test: malicious packages can try to escape the package root
  // using directories.bin to chmod files at arbitrary locations
  expect(
    await getBinsFromPackageManifest({
      name: 'malicious',
      version: '1.0.0',
      directories: {
        bin: '../../../../tmp/target',
      },
    }, process.cwd())).toStrictEqual([])

  expect(
    await getBinsFromPackageManifest({
      name: 'malicious',
      version: '1.0.0',
      directories: {
        bin: '../../../etc',
      },
    }, process.cwd())).toStrictEqual([])
})

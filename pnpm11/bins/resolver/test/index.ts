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

test('normalize parent segments in bin locations', async () => {
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
        name: 'bad',
        path: path.resolve('bad'),
      },
      {
        name: 'good',
        path: path.resolve('good'),
      },
    ]
  )
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

test('skip reserved bin names', async () => {
  expect(
    await getBinsFromPackageManifest({
      name: 'malicious',
      version: '1.0.0',
      bin: {
        '': './empty.js',
        '.': './dot.js',
        '..': './dot-dot.js',
        '@scope/..': './scoped-dot-dot.js',
        good: './good',
      },
    }, process.cwd())).toStrictEqual([
    {
      name: 'good',
      path: path.resolve('good'),
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

test.each(['../dist/cli.js', '../../dist/cli.js', 'lib/../../dist/cli.js'])('clamp bin target %s inside the package', async (target) => {
  const pkgPath = path.resolve('pkg')
  const results = await Promise.all([target, { tool: target }].map(async (bin) =>
    getBinsFromPackageManifest({ name: 'tool', version: '1.0.0', bin }, pkgPath)))
  for (const bins of results) {
    expect(bins).toStrictEqual([{ name: 'tool', path: path.join(pkgPath, 'dist/cli.js') }])
  }
})

test.each(['/outside.js', 'C:\\outside.js', '\\outside.js', 'C:cli.js', './C:cli.js', 'bin/../C:cli.js', '.', '..', 'bin/..'])('reject unsafe or empty normalized bin target %s', async (target) => {
  expect(await getBinsFromPackageManifest({ name: 'tool', version: '1.0.0', bin: { tool: target } }, path.resolve('pkg'))).toStrictEqual([])
})

test.each(['dist\\cli.js', '..\\dist\\cli.js', 'lib\\..\\..\\dist\\cli.js'])('normalize backslash bin target %s inside the package', async (target) => {
  const pkgPath = path.resolve('pkg')
  expect(await getBinsFromPackageManifest({ name: 'tool', version: '1.0.0', bin: { tool: target } }, pkgPath)).toStrictEqual([
    { name: 'tool', path: path.join(pkgPath, 'dist/cli.js') },
  ])
})

test('preserve colons in package-relative bin filenames', async () => {
  const pkgPath = path.resolve('pkg')
  expect(await getBinsFromPackageManifest({ name: 'tool', version: '1.0.0', bin: { tool: '../dist/cli:dev.js' } }, pkgPath)).toStrictEqual([
    { name: 'tool', path: path.join(pkgPath, 'dist/cli:dev.js') },
  ])
})

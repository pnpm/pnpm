import path from 'path'
import { getBinsFromPackageManifest } from '@pnpm/package-bins'

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
  const fixtures = path.join(__dirname, 'fixtures')
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

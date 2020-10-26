import getBinsFromPkg from '@pnpm/package-bins'
import path = require('path')

test('getBinsFromPkg()', async () => {
  expect(
    await getBinsFromPkg({
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

test('get bin of scoped package', async () => {
  expect(
    await getBinsFromPkg({
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
    await getBinsFromPkg({
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
    await getBinsFromPkg({
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

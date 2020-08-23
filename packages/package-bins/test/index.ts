import getBinsFromPkg from '@pnpm/package-bins'
import path = require('path')
import test = require('tape')

test('getBinsFromPkg()', async (t) => {
  t.deepEqual(
    await getBinsFromPkg({
      bin: 'one-bin',
      name: 'one-bin',
      version: '1.0.0',
    }, process.cwd()),
    [{
      name: 'one-bin',
      path: path.resolve('one-bin'),
    }]
  )
  t.end()
})

test('get bin of scoped package', async (t) => {
  t.deepEqual(
    await getBinsFromPkg({
      bin: 'bin.js',
      name: '@foo/bar',
      version: '1.0.0',
    }, process.cwd()),
    [{
      name: 'bar',
      path: path.resolve('bin.js'),
    }]
  )
  t.end()
})

test('skip dangerous bin names', async (t) => {
  t.deepEqual(
    await getBinsFromPkg({
      name: 'foo',
      version: '1.0.0',

      bin: {
        '../bad': './bad',
        '..\\bad': './bad',
        good: './good',
        '~/bad': './bad',
      },
    }, process.cwd()),
    [
      {
        name: 'good',
        path: path.resolve('good'),
      },
    ]
  )
  t.end()
})

test('skip dangerous bin locations', async (t) => {
  t.deepEqual(
    await getBinsFromPkg({
      name: 'foo',
      version: '1.0.0',

      bin: {
        bad: '../bad',
        good: './good',
      },
    }, process.cwd()),
    [
      {
        name: 'good',
        path: path.resolve('good'),
      },
    ]
  )
  t.end()
})

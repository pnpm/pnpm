import getBinsFromPkg from '@pnpm/package-bins'
import test = require('tape')
import path = require('path')

test('getBinsFromPkg()', async (t) => {
  t.deepEqual(
    await getBinsFromPkg({
      name: 'one-bin',
      version: '1.0.0',
      bin: 'one-bin'
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
      name: '@foo/bar',
      version: '1.0.0',
      bin: 'bin.js'
    }, process.cwd()),
    [{
      name: 'bar',
      path: path.resolve('bin.js'),
    }]
  )
  t.end()
})

test("skip dangerous bin names", async (t) => {
  t.deepEqual(
    await getBinsFromPkg({
      name: 'foo',
      version: '1.0.0',
      bin: {
        'good': './good',
        '../bad': './bad',
        '~/bad': './bad',
        '..\\bad': './bad',
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

test("skip dangerous bin locations", async (t) => {
  t.deepEqual(
    await getBinsFromPkg({
      name: 'foo',
      version: '1.0.0',
      bin: {
        'good': './good',
        'bad': '../bad',
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

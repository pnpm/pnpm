import createFuseHandlers from '../src/createFuseHandlers'
import path = require('path')
import Fuse = require('fuse-native')

test('readdir', async () => {
  const fixture = path.join(__dirname, '__fixtures__/simple')
  const { getattr, readdir } = await createFuseHandlers(fixture, path.join(fixture, 'store/v3/files'))

  expect.assertions(25)

  readdir('/', (returnCode, files) => {
    expect(returnCode).toBe(0)
    expect(files).toStrictEqual([
      '.pnpm',
      '@zkochan',
      'is-positive',
    ])
  })
  readdir('/.pnpm', (returnCode, files) => {
    expect(returnCode).toBe(0)
    expect(files).toStrictEqual([
      '@zkochan',
      'ini@1.3.8',
      'is-positive@1.0.0',
    ])
  })
  readdir('/.pnpm/is-positive@1.0.0', (returnCode, files) => {
    expect(returnCode).toBe(0)
    expect(files).toStrictEqual(['node_modules'])
  })
  readdir('/.pnpm/is-positive@1.0.0/node_modules', (returnCode, files) => {
    expect(returnCode).toBe(0)
    expect(files).toStrictEqual(['is-positive'])
  })
  readdir('/.pnpm/@zkochan/git-config@0.1.0/node_modules/@zkochan', (returnCode, files) => {
    expect(returnCode).toBe(0)
    expect(files).toStrictEqual(['git-config'])
  })
  readdir('/.pnpm/@zkochan/git-config@0.1.0/node_modules/@zkochan/git-config', (returnCode, files) => {
    expect(returnCode).toBe(0)
    expect(files).toStrictEqual([
      'package.json',
      '.npmignore',
      'README.md',
      'LICENSE',
      'Gruntfile.js',
      '.travis.yml',
      '.jshintrc',
      'test',
      'index.js',
    ])
  })
  getattr('/.pnpm/@zkochan/git-config@0.1.0/node_modules/@zkochan/git-config/index.js', (returnCode, stat) => {
    expect(returnCode).toBe(0)
    expect(stat.mode).toBe(33188)
  })
  getattr('/.pnpm/@zkochan/git-config@0.1.0/node_modules/@zkochan/git-config/test/fixtures', (returnCode, stat) => {
    expect(returnCode).toBe(0)
    expect(stat.mode).toBe(16877)
  })
  getattr('/.pnpm/@zkochan/git-config@0.1.0/node_modules/@zkochan/git-config/index.jsx', (returnCode, stat) => {
    expect(returnCode).toBe(Fuse.ENOENT)
  })
  readdir('/.pnpm/@zkochan/git-config@0.1.0/node_modules/@zkochan/git-config/test', (returnCode, files) => {
    expect(returnCode).toBe(0)
    expect(files).toStrictEqual([
      'index.js',
      'fixtures',
    ])
  })
  readdir('/.pnpm/@zkochan/git-config@0.1.0/node_modules/@zkochan/git-config/does-not-exist', (returnCode, files) => {
    expect(returnCode).toBe(Fuse.ENOENT)
  })
  readdir('/.pnpm/is-positive@1.0.0/node_modules/is-positive', (returnCode, files) => {
    expect(returnCode).toBe(0)
    expect(files).toStrictEqual([
      'package.json',
      'index.js',
      'license',
      'readme.md',
    ])
  })
  getattr('/.pnpm/is-positive@1.0.0/node_modules/is-positive/package.json', (returnCode, stat) => {
    expect(returnCode).toBe(0)
    expect(stat.mode).toBe(33188)
  })
  readdir('/.pnpm/@zkochan/git-config@0.1.0/node_modules/@types', (returnCode) => {
    expect(returnCode).toBe(Fuse.ENOENT)
  })
})

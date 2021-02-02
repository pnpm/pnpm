import path = require('path')

jest.mock('fuse-native', () => ({ ENOENT: -2 }))

// eslint-disable-next-line
import createFuseHandlers, { createFuseHandlersFromLockfile } from '../src/createFuseHandlers'
// eslint-disable-next-line
import Fuse = require('fuse-native')

describe('FUSE handlers', () => {
  let handlers: ReturnType<typeof createFuseHandlersFromLockfile>
  beforeAll(async () => {
    const fixture = path.join(__dirname, '__fixtures__/simple')
    handlers = await createFuseHandlers(fixture, path.join(fixture, 'store/v3/files'))
  })

  it('readdir', () => {
    handlers.readdir('/', (returnCode, files) => {
      expect(returnCode).toBe(0)
      expect(files!.sort()).toStrictEqual([
        '.pnpm',
        '@zkochan',
        'is-positive',
      ].sort())
    })
    handlers.readdir('/.pnpm', (returnCode, files) => {
      expect(returnCode).toBe(0)
      expect(files!.sort()).toStrictEqual([
        '@zkochan#git-config@0.1.0',
        'ini@1.3.8',
        'is-positive@1.0.0',
      ].sort())
    })
    handlers.readdir('/.pnpm/is-positive@1.0.0', (returnCode, files) => {
      expect(returnCode).toBe(0)
      expect(files).toStrictEqual(['node_modules'])
    })
    handlers.readdir('/.pnpm/is-positive@1.0.0/node_modules', (returnCode, files) => {
      expect(returnCode).toBe(0)
      expect(files).toStrictEqual(['is-positive'])
    })
    handlers.readdir('/.pnpm/@zkochan#git-config@0.1.0/node_modules/@zkochan', (returnCode, files) => {
      expect(returnCode).toBe(0)
      expect(files).toStrictEqual(['git-config'])
    })
    handlers.readdir('/.pnpm/@zkochan#git-config@0.1.0/node_modules/@zkochan/git-config', (returnCode, files) => {
      expect(returnCode).toBe(0)
      expect(files!.sort()).toStrictEqual([
        'package.json',
        '.npmignore',
        'README.md',
        'LICENSE',
        'Gruntfile.js',
        '.travis.yml',
        '.jshintrc',
        'test',
        'index.js',
      ].sort())
    })
    handlers.readdir('/.pnpm/@zkochan#git-config@0.1.0/node_modules/@zkochan/git-config/test', (returnCode, files) => {
      expect(returnCode).toBe(0)
      expect(files!.sort()).toStrictEqual([
        'index.js',
        'fixtures',
      ].sort())
    })
    handlers.readdir('/.pnpm/@zkochan#git-config@0.1.0/node_modules/@zkochan/git-config/does-not-exist', (returnCode, files) => {
      expect(returnCode).toBe(Fuse.ENOENT)
    })
    handlers.readdir('/.pnpm/is-positive@1.0.0/node_modules/is-positive', (returnCode, files) => {
      expect(returnCode).toBe(0)
      expect(files!.sort()).toStrictEqual([
        'package.json',
        'index.js',
        'license',
        'readme.md',
      ].sort())
    })
    handlers.readdir('/.pnpm/@zkochan#git-config@0.1.0/node_modules/@types', (returnCode) => {
      expect(returnCode).toBe(Fuse.ENOENT)
    })
  })
  it('getattr', () => {
    handlers.getattr('/.pnpm/@zkochan#git-config@0.1.0/node_modules/@zkochan/git-config/index.js', (returnCode, stat) => {
      expect(returnCode).toBe(0)
      expect(stat.mode).toBe(33188)
    })
    handlers.getattr('/.pnpm/@zkochan#git-config@0.1.0/node_modules/@zkochan/git-config/test/fixtures', (returnCode, stat) => {
      expect(returnCode).toBe(0)
      expect(stat.mode).toBe(16877)
    })
    handlers.getattr('/.pnpm/@zkochan#git-config@0.1.0/node_modules/@zkochan/git-config/index.jsx', (returnCode, stat) => {
      expect(returnCode).toBe(Fuse.ENOENT)
    })
    handlers.getattr('/.pnpm/is-positive@1.0.0/node_modules/is-positive/package.json', (returnCode, stat) => {
      expect(returnCode).toBe(0)
      expect(stat.mode).toBe(33188)
    })
  })
  it('open and read', (done) => {
    const p = '/.pnpm/@zkochan#git-config@0.1.0/node_modules/@zkochan/git-config/index.js'
    handlers.open(p, 0, (exitCode, fd) => {
      expect(exitCode).toBe(0)
      expect(fd && fd > 0).toBeTruthy()
      const buffer = Buffer.alloc(10)

      handlers.read(p, fd!, buffer, 10, 0, (readBytes) => {
        expect(readBytes).toBe(10)
        expect(buffer.toString()).toBe('var ini = ')

        handlers.release(p, fd!, (exitCode) => {
          expect(exitCode).toBe(0)
          done()
        })
      })
    })
  })
})

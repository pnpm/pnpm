import path from 'path'
import { type Config } from '../src/Config'
import { transformPath, transformPathKeys } from '../src/transformPath'

describe('transformPath', () => {
  test('replaces starting tilde with homedir', () => {
    expect(transformPath('~/.local/share/pnpm', '/home/user')).toBe(path.join('/home/user', '.local/share/pnpm'))
    expect(transformPath('~\\.local\\share\\pnpm', 'C:\\Users\\user')).toBe(path.join('C:\\Users\\user', '.local\\share\\pnpm'))
  })

  test('leaves non leading tilde as-is', () => {
    expect(transformPath('foo/bar/~/baz', '/home/user')).toBe('foo/bar/~/baz')
  })

  test('leaves leading tilde not being followed by separator as-is', () => {
    expect(transformPath('~foo/bar/baz', '/home/user')).toBe('~foo/bar/baz')
  })
})

test('transformPathKeys', () => {
  const config: Partial<Config> = {
    cacheDir: '~/.cache/pnpm',
    storeDir: '~/.local/share/pnpm',
  }
  transformPathKeys(config, '/home/user')
  expect(config).toStrictEqual({
    cacheDir: path.join('/home/user', '.cache/pnpm'),
    storeDir: path.join('/home/user', '.local/share/pnpm'),
  })
})

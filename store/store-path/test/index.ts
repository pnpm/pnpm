import path from 'path'
import { STORE_VERSION } from '@pnpm/constants'
import { jest } from '@jest/globals'
import isWindows from 'is-windows'

jest.unstable_mockModule('touch', () => {
  return {
    default: jest.fn(),
  }
})
jest.unstable_mockModule('root-link-target', () => {
  const MAPPINGS: Record<string, string> = {
    '/src/workspace/project/tmp': '/',
    '/mnt/project/tmp': '/mnt/project',
  }

  return {
    default: async function (file: string): Promise<string> {
      return MAPPINGS[file]
    },
  }
})
jest.unstable_mockModule('path-temp', () => {
  return {
    default: function (dir: string): string {
      return path.join(dir, 'tmp')
    },
  }
})
jest.unstable_mockModule('os', () => ({
  default: {
    homedir: () => '/home/user',
  },
}))
jest.unstable_mockModule('fs', () => ({
  promises: {
    mkdir: () => {},
    unlink: () => {},
    rmdir: () => {},
  },
}))
jest.unstable_mockModule('can-link', () => {
  const CAN_LINK = new Set([
    '/can-link-to-homedir/tmp=>/home/user/tmp',
    '/mnt/project/tmp=>/mnt/tmp/tmp',
  ])

  return {
    default: function (existingPath: string, newPath: string): boolean {
      return CAN_LINK.has(`${existingPath}=>${newPath}`)
    },
  }
})

const { getStorePath } = await import('@pnpm/store-path')

const skipOnWindows = isWindows() ? test.skip : test

skipOnWindows('when a link can be created to the homedir', async () => {
  expect(await getStorePath({
    pkgRoot: '/can-link-to-homedir',
    pnpmHomeDir: '/local/share/pnpm',
  })).toBe(`/local/share/pnpm/store/${STORE_VERSION}`)
})

skipOnWindows('a link can be created to the root of the drive', async () => {
  expect(await getStorePath({
    pkgRoot: '/src/workspace/project',
    pnpmHomeDir: '/local/share/pnpm',
  })).toBe(`/.pnpm-store/${STORE_VERSION}`)
})

skipOnWindows('a link can be created to the a subdir in the root of the drive', async () => {
  expect(await getStorePath({
    pkgRoot: '/mnt/project',
    pnpmHomeDir: '/local/share/pnpm',
  })).toBe(`/mnt/.pnpm-store/${STORE_VERSION}`)
})

test('fail when pnpm home directory is not defined', async () => {
  expect(() => getStorePath({
    pkgRoot: 'pkgRoot',
    // @ts-expect-error
    pnpmHomeDir: undefined,
  })).toThrow('The pnpm home directory is unknown. Cannot calculate the store directory location.')
})

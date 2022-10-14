import { getStorePath } from '@pnpm/store-path'
import isWindows from 'is-windows'

jest.mock('os')
jest.mock('fs')

const skipOnWindows = isWindows() ? test.skip : test

skipOnWindows('when a link can be created to the homedir', async () => {
  expect(await getStorePath({
    pkgRoot: '/can-link-to-homedir',
    pnpmHomeDir: '/local/share/pnpm',
  })).toBe('/local/share/pnpm/store/v3')
})

skipOnWindows('a link can be created to the root of the drive', async () => {
  expect(await getStorePath({
    pkgRoot: '/src/workspace/project',
    pnpmHomeDir: '/local/share/pnpm',
  })).toBe('/.pnpm-store/v3')
})

skipOnWindows('a link can be created to the a subdir in the root of the drive', async () => {
  expect(await getStorePath({
    pkgRoot: '/mnt/project',
    pnpmHomeDir: '/local/share/pnpm',
  })).toBe('/mnt/.pnpm-store/v3')
})

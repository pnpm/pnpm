import storePath from '@pnpm/store-path'
import isWindows from 'is-windows'

jest.mock('os')
jest.mock('fs')

const skipOnWindows = isWindows() ? test.skip : test

skipOnWindows('when a link can be created to the homedir', async () => {
  expect(await storePath('/can-link-to-homedir')).toBe('/home/user/.pnpm-store/v3')
})

skipOnWindows('a link can be created to the root of the drive', async () => {
  expect(await storePath('/src/workspace/project')).toBe('/.pnpm-store/v3')
})

skipOnWindows('a link can be created to the a subdir in the root of the drive', async () => {
  expect(await storePath('/mnt/project')).toBe('/mnt/.pnpm-store/v3')
})

import path from 'node:path'

import { beforeEach, expect, jest, test } from '@jest/globals'
import { STORE_VERSION } from '@pnpm/constants'

const DRIVE_ROOT = path.parse(process.cwd()).root
const CAN_LINK_HOME_PROJECT = path.join(DRIVE_ROOT, 'can-link-to-homedir')
const MOUNT_ROOT = path.join(DRIVE_ROOT, 'mnt')
const MOUNT_PROJECT = path.join(MOUNT_ROOT, 'project')
const PNPM_HOME_DIR = path.join(DRIVE_ROOT, 'local', 'share', 'pnpm')
const ROOT_PROJECT = path.join(DRIVE_ROOT, 'src', 'workspace', 'project')
const SANDBOX_ROOT = path.join(DRIVE_ROOT, 'sandbox')
const SANDBOX_PROJECT = path.join(SANDBOX_ROOT, 'project')

jest.unstable_mockModule('touch', () => {
  return {
    default: jest.fn(),
  }
})
jest.unstable_mockModule('root-link-target', () => {
  const MAPPINGS: Record<string, string> = {
    [path.join(MOUNT_PROJECT, 'tmp')]: MOUNT_PROJECT,
    [path.join(ROOT_PROJECT, 'tmp')]: DRIVE_ROOT,
    [path.join(SANDBOX_PROJECT, 'tmp')]: SANDBOX_PROJECT,
  }

  return {
    rootLinkTarget: async function (file: string): Promise<string> {
      return MAPPINGS[file]
    },
  }
})
jest.unstable_mockModule('path-temp', () => {
  return {
    pathTemp: function (dir: string): string {
      return path.join(dir, 'tmp')
    },
  }
})
jest.unstable_mockModule('os', () => ({
  default: {
    homedir: () => '/home/user',
  },
}))
const fsMock = {
  default: {
    promises: {
      mkdir: () => {},
      unlink: () => {},
      rmdir: () => {},
      rm: () => {},
    },
    rmSync: () => {},
  },
  promises: {
    mkdir: () => {},
    unlink: () => {},
    rmdir: () => {},
    rm: () => {},
  },
}
jest.unstable_mockModule('fs', () => fsMock)
jest.unstable_mockModule('node:fs', () => fsMock)
const CAN_LINK = new Set([
  `${path.join(CAN_LINK_HOME_PROJECT, 'tmp')}=>${path.join(PNPM_HOME_DIR, 'tmp', 'tmp')}`,
  `${path.join(MOUNT_PROJECT, 'tmp')}=>${path.join(MOUNT_ROOT, 'tmp', 'tmp')}`,
])
const canLinkMock = jest.fn(function (existingPath: string, newPath: string): boolean {
  return CAN_LINK.has(`${existingPath}=>${newPath}`)
})

jest.unstable_mockModule('can-link', () => {
  return {
    canLink: canLinkMock,
  }
})

const { getStorePath } = await import('@pnpm/store.path')

beforeEach(() => {
  canLinkMock.mockClear()
})

test('when a link can be created to the homedir', async () => {
  expect(await getStorePath({
    pkgRoot: CAN_LINK_HOME_PROJECT,
    pnpmHomeDir: PNPM_HOME_DIR,
  })).toBe(path.join(PNPM_HOME_DIR, 'store', STORE_VERSION))
  expect(canLinkMock).toHaveBeenCalledWith(
    path.join(CAN_LINK_HOME_PROJECT, 'tmp'),
    path.join(PNPM_HOME_DIR, 'tmp', 'tmp')
  )
})

test('a link can be created to the root of the drive', async () => {
  expect(await getStorePath({
    pkgRoot: ROOT_PROJECT,
    pnpmHomeDir: PNPM_HOME_DIR,
  })).toBe(path.join(DRIVE_ROOT, '.pnpm-store', STORE_VERSION))
  expect(canLinkMock).not.toHaveBeenCalledWith(
    path.join(ROOT_PROJECT, 'tmp'),
    path.join(DRIVE_ROOT, 'tmp', 'tmp')
  )
})

test('a link can be created to a subdir in the root of the drive', async () => {
  expect(await getStorePath({
    pkgRoot: MOUNT_PROJECT,
    pnpmHomeDir: PNPM_HOME_DIR,
  })).toBe(path.join(MOUNT_ROOT, '.pnpm-store', STORE_VERSION))
})

test('the store is created in the project when only the project directory is linkable', async () => {
  expect(await getStorePath({
    pkgRoot: SANDBOX_PROJECT,
    pnpmHomeDir: PNPM_HOME_DIR,
  })).toBe(path.join(SANDBOX_PROJECT, '.pnpm-store', STORE_VERSION))
  expect(canLinkMock).toHaveBeenCalledWith(
    path.join(SANDBOX_PROJECT, 'tmp'),
    path.join(SANDBOX_ROOT, 'tmp', 'tmp')
  )
})

test('fail when pnpm home directory is not defined', async () => {
  expect(() => getStorePath({
    pkgRoot: 'pkgRoot',
    // @ts-expect-error
    pnpmHomeDir: undefined,
  })).toThrow('The pnpm home directory is unknown. Cannot calculate the store directory location.')
})

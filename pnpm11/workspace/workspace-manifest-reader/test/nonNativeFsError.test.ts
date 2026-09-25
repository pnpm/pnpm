import { beforeEach, expect, jest, test } from '@jest/globals'

let errorCode = 'ENOENT'

jest.unstable_mockModule('read-yaml-file', () => ({
  readYamlFile: jest.fn(async () => {
    throw createNonNativeFsError(errorCode)
  }),
  readYamlFileSync: jest.fn(() => {
    throw createNonNativeFsError(errorCode)
  }),
}))

const { readWorkspaceManifest, readWorkspaceManifestSync } = await import('@pnpm/workspace.workspace-manifest-reader')

beforeEach(() => {
  errorCode = 'ENOENT'
})

test('readWorkspaceManifest() treats a non-native ENOENT error as a missing file', async () => {
  await expect(readWorkspaceManifest('/home/.config/pnpm', 'config.yaml')).resolves.toBeUndefined()
})

test('readWorkspaceManifestSync() treats a non-native ENOENT error as a missing file', () => {
  expect(readWorkspaceManifestSync('/home/.config/pnpm', 'config.yaml')).toBeUndefined()
})

test('readWorkspaceManifest() rethrows other non-native fs errors', async () => {
  errorCode = 'EACCES'
  await expect(readWorkspaceManifest('/home/.config/pnpm', 'config.yaml')).rejects.toMatchObject({ code: 'EACCES' })
})

test('readWorkspaceManifestSync() rethrows other non-native fs errors', () => {
  errorCode = 'EACCES'
  expect(() => readWorkspaceManifestSync('/home/.config/pnpm', 'config.yaml')).toThrow(expect.objectContaining({ code: 'EACCES' }))
})

// StackBlitz WebContainers throw fs errors that carry the usual errno fields
// but are not native errors (util.types.isNativeError() returns false).
function createNonNativeFsError (code: string): Error {
  return Object.assign(Object.create(Error.prototype) as Error, {
    code,
    errno: -2,
    path: '/home/.config/pnpm/config.yaml',
    syscall: 'open',
    message: `${code}: open '/home/.config/pnpm/config.yaml'`,
  })
}

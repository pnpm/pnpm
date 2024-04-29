import { readWorkspaceManifest } from '@pnpm/workspace.read-manifest'
import path from 'node:path'

test('readWorkspaceManifest() works with a valid workspace file', async () => {
  const manifest = await readWorkspaceManifest(path.join(__dirname, '__fixtures__/ok'))

  expect(manifest).toEqual({
    packages: ['packages/**', 'types'],
  })
})

test('readWorkspaceManifest() throws on string content', async () => {
  await expect(
    readWorkspaceManifest(path.join(__dirname, '__fixtures__/string'))
  ).rejects.toThrow('Expected object but found - string')
})

test('readWorkspaceManifest() throws on array content', async () => {
  await expect(
    readWorkspaceManifest(path.join(__dirname, '__fixtures__/array'))
  ).rejects.toThrow('Expected object but found - array')
})

test('readWorkspaceManifest() throws on empty packages field', async () => {
  await expect(
    readWorkspaceManifest(path.join(__dirname, '__fixtures__/packages-empty'))
  ).rejects.toThrow('packages field missing or empty')
})

test('readWorkspaceManifest() throws on string packages field', async () => {
  await expect(
    readWorkspaceManifest(path.join(__dirname, '__fixtures__/packages-string'))
  ).rejects.toThrow('packages field is not an array')
})

test('readWorkspaceManifest() throws on empty package', async () => {
  await expect(
    readWorkspaceManifest(path.join(__dirname, '__fixtures__/packages-contains-empty'))
  ).rejects.toThrow('Missing or empty package')
})

test('readWorkspaceManifest() throws on numeric package', async () => {
  await expect(
    readWorkspaceManifest(path.join(__dirname, '__fixtures__/packages-contains-number'))
  ).rejects.toThrow('Invalid package type - number')
})

test('readWorkspaceManifest() works when no workspace file is present', async () => {
  const manifest = await readWorkspaceManifest(path.join(__dirname, '__fixtures__/no-workspace-file'))

  expect(manifest).toBeUndefined()
})

test('readWorkspaceManifest() works when workspace file is empty', async () => {
  const manifest = await readWorkspaceManifest(path.join(__dirname, '__fixtures__/empty'))

  expect(manifest).toBeUndefined()
})

test('readWorkspaceManifest() works when workspace file is null', async () => {
  const manifest = await readWorkspaceManifest(path.join(__dirname, '__fixtures__/null'))

  expect(manifest).toBeNull()
})
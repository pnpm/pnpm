/// <reference path="../../../__typings__/index.d.ts"/>
import path from 'path'
import { resolveFromLocal } from '@pnpm/local-resolver'
import { type DirectoryResolution } from '@pnpm/resolver-base'
import normalize from 'normalize-path'
import { logger } from '@pnpm/logger'

const TEST_DIR = path.dirname(require.resolve('@pnpm/tgz-fixtures/tgz/pnpm-local-resolver-0.1.1.tgz'))

test('resolve directory', async () => {
  const resolveResult = await resolveFromLocal({}, { bareSpecifier: '..' }, { projectDir: __dirname })
  expect(resolveResult!.id).toEqual('link:..')
  expect(resolveResult!.normalizedBareSpecifier).toEqual('link:..')
  expect(resolveResult!['manifest']!.name).toEqual('@pnpm/local-resolver')
  expect((resolveResult!.resolution as DirectoryResolution).directory).toEqual(normalize(path.join(__dirname, '..')))
  expect((resolveResult!.resolution as DirectoryResolution).type).toEqual('directory')
})

test('resolve directory specified using absolute path', async () => {
  const linkedDir = path.join(__dirname, '..')
  const normalizedLinkedDir = normalize(linkedDir)
  const resolveResult = await resolveFromLocal({}, { bareSpecifier: `link:${linkedDir}` }, { projectDir: __dirname })
  expect(resolveResult!.id).toEqual('link:..')
  expect(resolveResult!.normalizedBareSpecifier).toEqual(`link:${normalizedLinkedDir}`)
  expect(resolveResult!['manifest']!.name).toEqual('@pnpm/local-resolver')
  expect((resolveResult!.resolution as DirectoryResolution).directory).toEqual(normalizedLinkedDir)
  expect((resolveResult!.resolution as DirectoryResolution).type).toEqual('directory')
})

test('resolve directory specified using absolute path with preserveAbsolutePaths', async () => {
  const linkedDir = path.join(__dirname, '..')
  const normalizedLinkedDir = normalize(linkedDir)
  const resolveResult = await resolveFromLocal({ preserveAbsolutePaths: true }, { bareSpecifier: `link:${linkedDir}` }, { projectDir: __dirname })
  expect(resolveResult!.id).toEqual(`link:${normalizedLinkedDir}`)
  expect(resolveResult!.normalizedBareSpecifier).toEqual(`link:${normalizedLinkedDir}`)
  expect(resolveResult!['manifest']!.name).toEqual('@pnpm/local-resolver')
  expect((resolveResult!.resolution as DirectoryResolution).directory).toEqual(normalizedLinkedDir)
  expect((resolveResult!.resolution as DirectoryResolution).type).toEqual('directory')
})

test('resolve directory specified using absolute path with preserveAbsolutePaths and file: scheme', async () => {
  const linkedDir = path.join(__dirname, '..')
  const normalizedLinkedDir = normalize(linkedDir)
  const resolveResult = await resolveFromLocal(
    { preserveAbsolutePaths: true },
    { bareSpecifier: `file:${linkedDir}` },
    { projectDir: __dirname }
  )
  expect(resolveResult!.id).toEqual(`file:${normalizedLinkedDir}`)
  expect(resolveResult!.normalizedBareSpecifier).toEqual(`file:${normalizedLinkedDir}`)
  expect(resolveResult!['manifest']!.name).toEqual('@pnpm/local-resolver')
  expect((resolveResult!.resolution as DirectoryResolution).directory).toEqual(normalizedLinkedDir)
  expect((resolveResult!.resolution as DirectoryResolution).type).toEqual('directory')
})

test('resolve injected directory', async () => {
  const resolveResult = await resolveFromLocal({}, { injected: true, bareSpecifier: '..' }, { projectDir: __dirname })
  expect(resolveResult!.id).toEqual('file:..')
  expect(resolveResult!.normalizedBareSpecifier).toEqual('file:..')
  expect(resolveResult!['manifest']!.name).toEqual('@pnpm/local-resolver')
  expect((resolveResult!.resolution as DirectoryResolution).directory).toEqual('..')
  expect((resolveResult!.resolution as DirectoryResolution).type).toEqual('directory')
})

test('resolve workspace directory', async () => {
  const resolveResult = await resolveFromLocal({}, { bareSpecifier: 'workspace:..' }, { projectDir: __dirname })
  expect(resolveResult!.id).toEqual('link:..')
  expect(resolveResult!.normalizedBareSpecifier).toEqual('link:..')
  expect(resolveResult!['manifest']!.name).toEqual('@pnpm/local-resolver')
  expect((resolveResult!.resolution as DirectoryResolution).directory).toEqual(normalize(path.join(__dirname, '..')))
  expect((resolveResult!.resolution as DirectoryResolution).type).toEqual('directory')
})

test('resolve directory specified using the file: protocol', async () => {
  const resolveResult = await resolveFromLocal({}, { bareSpecifier: 'file:..' }, { projectDir: __dirname })
  expect(resolveResult!.id).toEqual('file:..')
  expect(resolveResult!.normalizedBareSpecifier).toEqual('file:..')
  expect(resolveResult!['manifest']!.name).toEqual('@pnpm/local-resolver')
  expect((resolveResult!.resolution as DirectoryResolution).directory).toEqual('..')
  expect((resolveResult!.resolution as DirectoryResolution).type).toEqual('directory')
})

test('resolve directory specified using the link: protocol', async () => {
  const resolveResult = await resolveFromLocal({}, { bareSpecifier: 'link:..' }, { projectDir: __dirname })
  expect(resolveResult!.id).toEqual('link:..')
  expect(resolveResult!.normalizedBareSpecifier).toEqual('link:..')
  expect(resolveResult!['manifest']!.name).toEqual('@pnpm/local-resolver')
  expect((resolveResult!.resolution as DirectoryResolution).directory).toEqual(normalize(path.join(__dirname, '..')))
  expect((resolveResult!.resolution as DirectoryResolution).type).toEqual('directory')
})

test('resolve file', async () => {
  const wantedDependency = { bareSpecifier: './pnpm-local-resolver-0.1.1.tgz' }
  const resolveResult = await resolveFromLocal({}, wantedDependency, { projectDir: TEST_DIR })

  expect(resolveResult).toEqual({
    id: 'file:pnpm-local-resolver-0.1.1.tgz',
    normalizedBareSpecifier: 'file:pnpm-local-resolver-0.1.1.tgz',
    resolution: {
      integrity: 'sha512-UHd2zKRT/w70KKzFlj4qcT81A1Q0H7NM9uKxLzIZ/VZqJXzt5Hnnp2PYPb5Ezq/hAamoYKIn5g7fuv69kP258w==',
      tarball: 'file:pnpm-local-resolver-0.1.1.tgz',
    },
    resolvedVia: 'local-filesystem',
  })
})

test("resolve file when lockfile directory differs from the package's dir", async () => {
  const wantedDependency = { bareSpecifier: './pnpm-local-resolver-0.1.1.tgz' }
  const resolveResult = await resolveFromLocal({}, wantedDependency, {
    lockfileDir: path.join(TEST_DIR, '..'),
    projectDir: TEST_DIR,
  })

  expect(resolveResult).toEqual({
    id: 'file:tgz/pnpm-local-resolver-0.1.1.tgz',
    normalizedBareSpecifier: 'file:pnpm-local-resolver-0.1.1.tgz',
    resolution: {
      integrity: 'sha512-UHd2zKRT/w70KKzFlj4qcT81A1Q0H7NM9uKxLzIZ/VZqJXzt5Hnnp2PYPb5Ezq/hAamoYKIn5g7fuv69kP258w==',
      tarball: 'file:tgz/pnpm-local-resolver-0.1.1.tgz',
    },
    resolvedVia: 'local-filesystem',
  })
})

test('resolve tarball specified with file: protocol', async () => {
  const wantedDependency = { bareSpecifier: 'file:./pnpm-local-resolver-0.1.1.tgz' }
  const resolveResult = await resolveFromLocal({}, wantedDependency, { projectDir: TEST_DIR })

  expect(resolveResult).toEqual({
    id: 'file:pnpm-local-resolver-0.1.1.tgz',
    normalizedBareSpecifier: 'file:pnpm-local-resolver-0.1.1.tgz',
    resolution: {
      integrity: 'sha512-UHd2zKRT/w70KKzFlj4qcT81A1Q0H7NM9uKxLzIZ/VZqJXzt5Hnnp2PYPb5Ezq/hAamoYKIn5g7fuv69kP258w==',
      tarball: 'file:pnpm-local-resolver-0.1.1.tgz',
    },
    resolvedVia: 'local-filesystem',
  })
})

test('fail when resolving tarball specified with the link: protocol', async () => {
  try {
    const wantedDependency = { bareSpecifier: 'link:./pnpm-local-resolver-0.1.1.tgz' }
    await resolveFromLocal({}, wantedDependency, { projectDir: TEST_DIR })
    fail()
  } catch (err: any) { // eslint-disable-line
    expect(err).toBeDefined()
    expect(err.code).toEqual('ERR_PNPM_NOT_PACKAGE_DIRECTORY')
  }
})

test('fail when resolving from not existing directory an injected dependency', async () => {
  const wantedDependency = { bareSpecifier: 'file:./dir-does-not-exist' }
  const projectDir = __dirname
  await expect(
    resolveFromLocal({}, wantedDependency, { projectDir })
  ).rejects.toThrow(`Could not install from "${path.join(projectDir, 'dir-does-not-exist')}" as it does not exist.`)
})

test('do not fail when resolving from not existing directory', async () => {
  jest.spyOn(logger, 'warn')
  const wantedDependency = { bareSpecifier: 'link:./dir-does-not-exist' }
  const resolveResult = await resolveFromLocal({}, wantedDependency, { projectDir: __dirname })
  expect(resolveResult?.manifest).toStrictEqual({
    name: 'dir-does-not-exist',
    version: '0.0.0',
  })
  expect(logger.warn).toHaveBeenCalledWith({
    message: `Installing a dependency from a non-existent directory: ${path.join(__dirname, './dir-does-not-exist')}`,
    prefix: __dirname,
  })
  ;(logger.warn as jest.Mock).mockRestore()
})

test('throw error when the path: protocol is used', async () => {
  try {
    await resolveFromLocal({}, { bareSpecifier: 'path:..' }, { projectDir: __dirname })
    fail()
  } catch (err: any) { // eslint-disable-line
    expect(err).toBeDefined()
    expect(err.code).toEqual('ERR_PNPM_PATH_IS_UNSUPPORTED_PROTOCOL')
    expect(err.bareSpecifier).toEqual('path:..')
    expect(err.protocol).toEqual('path:')
  }
})

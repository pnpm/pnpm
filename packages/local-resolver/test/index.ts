/// <reference path="../../../typings/index.d.ts"/>
import resolveFromLocal from '@pnpm/local-resolver'
import path = require('path')

test('resolve directory', async () => {
  const resolveResult = await resolveFromLocal({ pref: '..' }, { projectDir: __dirname })
  expect(resolveResult!.id).toEqual('link:..')
  expect(resolveResult!.normalizedPref).toEqual('link:..')
  expect(resolveResult!['manifest']!.name).toEqual('@pnpm/local-resolver')
  expect(resolveResult!.resolution['directory']).toEqual('..')
  expect(resolveResult!.resolution['type']).toEqual('directory')
})

test('resolve workspace directory', async () => {
  const resolveResult = await resolveFromLocal({ pref: 'workspace:..' }, { projectDir: __dirname })
  expect(resolveResult!.id).toEqual('link:..')
  expect(resolveResult!.normalizedPref).toEqual('link:..')
  expect(resolveResult!['manifest']!.name).toEqual('@pnpm/local-resolver')
  expect(resolveResult!.resolution['directory']).toEqual('..')
  expect(resolveResult!.resolution['type']).toEqual('directory')
})

test('resolve directory specified using the file: protocol', async () => {
  const resolveResult = await resolveFromLocal({ pref: 'file:..' }, { projectDir: __dirname })
  expect(resolveResult!.id).toEqual('link:..')
  expect(resolveResult!.normalizedPref).toEqual('link:..')
  expect(resolveResult!['manifest']!.name).toEqual('@pnpm/local-resolver')
  expect(resolveResult!.resolution['directory']).toEqual('..')
  expect(resolveResult!.resolution['type']).toEqual('directory')
})

test('resolve directoty specified using the link: protocol', async () => {
  const resolveResult = await resolveFromLocal({ pref: 'link:..' }, { projectDir: __dirname })
  expect(resolveResult!.id).toEqual('link:..')
  expect(resolveResult!.normalizedPref).toEqual('link:..')
  expect(resolveResult!['manifest']!.name).toEqual('@pnpm/local-resolver')
  expect(resolveResult!.resolution['directory']).toEqual('..')
  expect(resolveResult!.resolution['type']).toEqual('directory')
})

test('resolve file', async () => {
  const wantedDependency = { pref: './pnpm-local-resolver-0.1.1.tgz' }
  const resolveResult = await resolveFromLocal(wantedDependency, { projectDir: __dirname })

  expect(resolveResult).toEqual({
    id: 'file:pnpm-local-resolver-0.1.1.tgz',
    normalizedPref: 'file:pnpm-local-resolver-0.1.1.tgz',
    resolution: {
      integrity: 'sha512-UHd2zKRT/w70KKzFlj4qcT81A1Q0H7NM9uKxLzIZ/VZqJXzt5Hnnp2PYPb5Ezq/hAamoYKIn5g7fuv69kP258w==',
      tarball: 'file:pnpm-local-resolver-0.1.1.tgz',
    },
    resolvedVia: 'local-filesystem',
  })
})

test("resolve file when lockfile directory differs from the package's dir", async () => {
  const wantedDependency = { pref: './pnpm-local-resolver-0.1.1.tgz' }
  const resolveResult = await resolveFromLocal(wantedDependency, {
    lockfileDir: path.join(__dirname, '..'),
    projectDir: __dirname,
  })

  expect(resolveResult).toEqual({
    id: 'file:test/pnpm-local-resolver-0.1.1.tgz',
    normalizedPref: 'file:pnpm-local-resolver-0.1.1.tgz',
    resolution: {
      integrity: 'sha512-UHd2zKRT/w70KKzFlj4qcT81A1Q0H7NM9uKxLzIZ/VZqJXzt5Hnnp2PYPb5Ezq/hAamoYKIn5g7fuv69kP258w==',
      tarball: 'file:test/pnpm-local-resolver-0.1.1.tgz',
    },
    resolvedVia: 'local-filesystem',
  })
})

test('resolve tarball specified with file: protocol', async () => {
  const wantedDependency = { pref: 'file:./pnpm-local-resolver-0.1.1.tgz' }
  const resolveResult = await resolveFromLocal(wantedDependency, { projectDir: __dirname })

  expect(resolveResult).toEqual({
    id: 'file:pnpm-local-resolver-0.1.1.tgz',
    normalizedPref: 'file:pnpm-local-resolver-0.1.1.tgz',
    resolution: {
      integrity: 'sha512-UHd2zKRT/w70KKzFlj4qcT81A1Q0H7NM9uKxLzIZ/VZqJXzt5Hnnp2PYPb5Ezq/hAamoYKIn5g7fuv69kP258w==',
      tarball: 'file:pnpm-local-resolver-0.1.1.tgz',
    },
    resolvedVia: 'local-filesystem',
  })
})

test('fail when resolving tarball specified with the link: protocol', async () => {
  try {
    const wantedDependency = { pref: 'link:./pnpm-local-resolver-0.1.1.tgz' }
    await resolveFromLocal(wantedDependency, { projectDir: __dirname })
    fail()
  } catch (err) {
    expect(err).toBeDefined()
    expect(err.code).toEqual('ERR_PNPM_NOT_PACKAGE_DIRECTORY')
  }
})

test('fail when resolving from not existing directory', async () => {
  try {
    const wantedDependency = { pref: 'link:./dir-does-not-exist' }
    await resolveFromLocal(wantedDependency, { projectDir: __dirname })
    fail()
  } catch (err) {
    expect(err).toBeDefined()
    expect(err.code).toEqual('ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND')
  }
})

test('throw error when the path: protocol is used', async () => {
  try {
    await resolveFromLocal({ pref: 'path:..' }, { projectDir: __dirname })
    fail()
  } catch (err) {
    expect(err).toBeDefined()
    expect(err.code).toEqual('ERR_PNPM_PATH_IS_UNSUPPORTED_PROTOCOL')
    expect(err.pref).toEqual('path:..')
    expect(err.protocol).toEqual('path:')
  }
})

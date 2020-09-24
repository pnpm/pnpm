/// <reference path="../../../typings/index.d.ts"/>
import resolveFromLocal from '@pnpm/local-resolver'
import path = require('path')
import test = require('tape')

test('resolve directory', async t => {
  const resolveResult = await resolveFromLocal({ pref: '..' }, { projectDir: __dirname })
  t.equal(resolveResult!.id, 'link:..')
  t.equal(resolveResult!.normalizedPref, 'link:..')
  t.equal(resolveResult!['manifest']!.name, '@pnpm/local-resolver')
  t.equal(resolveResult!.resolution['directory'], '..')
  t.equal(resolveResult!.resolution['type'], 'directory')
  t.end()
})

test('resolve directory specified using the file: protocol', async t => {
  const resolveResult = await resolveFromLocal({ pref: 'file:..' }, { projectDir: __dirname })
  t.equal(resolveResult!.id, 'link:..')
  t.equal(resolveResult!.normalizedPref, 'link:..')
  t.equal(resolveResult!['manifest']!.name, '@pnpm/local-resolver')
  t.equal(resolveResult!.resolution['directory'], '..')
  t.equal(resolveResult!.resolution['type'], 'directory')
  t.end()
})

test('resolve directoty specified using the link: protocol', async t => {
  const resolveResult = await resolveFromLocal({ pref: 'link:..' }, { projectDir: __dirname })
  t.equal(resolveResult!.id, 'link:..')
  t.equal(resolveResult!.normalizedPref, 'link:..')
  t.equal(resolveResult!['manifest']!.name, '@pnpm/local-resolver')
  t.equal(resolveResult!.resolution['directory'], '..')
  t.equal(resolveResult!.resolution['type'], 'directory')
  t.end()
})

test('resolve file', async t => {
  const wantedDependency = { pref: './pnpm-local-resolver-0.1.1.tgz' }
  const resolveResult = await resolveFromLocal(wantedDependency, { projectDir: __dirname })

  t.deepEqual(resolveResult, {
    id: 'file:pnpm-local-resolver-0.1.1.tgz',
    normalizedPref: 'file:pnpm-local-resolver-0.1.1.tgz',
    resolution: {
      integrity: 'sha512-UHd2zKRT/w70KKzFlj4qcT81A1Q0H7NM9uKxLzIZ/VZqJXzt5Hnnp2PYPb5Ezq/hAamoYKIn5g7fuv69kP258w==',
      tarball: 'file:pnpm-local-resolver-0.1.1.tgz',
    },
    resolvedVia: 'local-filesystem',
  })

  t.end()
})

test("resolve file when lockfile directory differs from the package's dir", async t => {
  const wantedDependency = { pref: './pnpm-local-resolver-0.1.1.tgz' }
  const resolveResult = await resolveFromLocal(wantedDependency, {
    lockfileDir: path.join(__dirname, '..'),
    projectDir: __dirname,
  })

  t.deepEqual(resolveResult, {
    id: 'file:test/pnpm-local-resolver-0.1.1.tgz',
    normalizedPref: 'file:pnpm-local-resolver-0.1.1.tgz',
    resolution: {
      integrity: 'sha512-UHd2zKRT/w70KKzFlj4qcT81A1Q0H7NM9uKxLzIZ/VZqJXzt5Hnnp2PYPb5Ezq/hAamoYKIn5g7fuv69kP258w==',
      tarball: 'file:test/pnpm-local-resolver-0.1.1.tgz',
    },
    resolvedVia: 'local-filesystem',
  })

  t.end()
})

test('resolve tarball specified with file: protocol', async t => {
  const wantedDependency = { pref: 'file:./pnpm-local-resolver-0.1.1.tgz' }
  const resolveResult = await resolveFromLocal(wantedDependency, { projectDir: __dirname })

  t.deepEqual(resolveResult, {
    id: 'file:pnpm-local-resolver-0.1.1.tgz',
    normalizedPref: 'file:pnpm-local-resolver-0.1.1.tgz',
    resolution: {
      integrity: 'sha512-UHd2zKRT/w70KKzFlj4qcT81A1Q0H7NM9uKxLzIZ/VZqJXzt5Hnnp2PYPb5Ezq/hAamoYKIn5g7fuv69kP258w==',
      tarball: 'file:pnpm-local-resolver-0.1.1.tgz',
    },
    resolvedVia: 'local-filesystem',
  })

  t.end()
})

test('fail when resolving tarball specified with the link: protocol', async t => {
  try {
    const wantedDependency = { pref: 'link:./pnpm-local-resolver-0.1.1.tgz' }
    await resolveFromLocal(wantedDependency, { projectDir: __dirname })
    t.fail()
  } catch (err) {
    t.ok(err)
    t.equal(err.code, 'ERR_PNPM_NOT_PACKAGE_DIRECTORY')
    t.end()
  }
})

test('fail when resolving from not existing directory', async t => {
  try {
    const wantedDependency = { pref: 'link:./dir-does-not-exist' }
    await resolveFromLocal(wantedDependency, { projectDir: __dirname })
    t.fail()
  } catch (err) {
    t.ok(err)
    t.equal(err.code, 'ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND')
    t.end()
  }
})

test('throw error when the path: protocol is used', async t => {
  try {
    await resolveFromLocal({ pref: 'path:..' }, { projectDir: __dirname })
    t.fail()
  } catch (err) {
    t.ok(err)
    t.equal(err.code, 'ERR_PNPM_PATH_IS_UNSUPPORTED_PROTOCOL')
    t.equal(err.pref, 'path:..')
    t.equal(err.protocol, 'path:')
    t.end()
  }
})

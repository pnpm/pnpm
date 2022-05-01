import fs from 'fs'
import path from 'path'
import {
  addDependenciesToPackage,
  install,
  link,
  mutateModules,
} from '@pnpm/core'
import { prepareEmpty } from '@pnpm/prepare'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { addDistTag } from '@pnpm/registry-mock'
import exists from 'path-exists'
import sinon from 'sinon'
import writeJsonFile from 'write-json-file'
import isInnerLink from 'is-inner-link'
import { testDefaults } from './utils'

test('unlink 1 package that exists in package.json', async () => {
  const project = prepareEmpty()
  process.chdir('..')

  await Promise.all([
    writeJsonFile('is-subdir/package.json', {
      dependencies: {
        'is-windows': '^1.0.0',
      },
      name: 'is-subdir',
      version: '1.0.0',
    }),
    writeJsonFile('is-positive/package.json', {
      name: 'is-positive',
      version: '1.0.0',
    }),
  ])

  const opts = await testDefaults({ fastUnpack: false, store: path.resolve('.store') })

  let manifest = await link(
    ['is-subdir', 'is-positive'],
    path.join('project', 'node_modules'),
    {
      ...opts,
      dir: path.resolve('project'),
      manifest: {
        dependencies: {
          'is-positive': '^1.0.0',
          'is-subdir': '^1.0.0',
        },
      },
    }
  )

  process.chdir('project')

  manifest = await install(manifest, opts)

  await mutateModules(
    [
      {
        dependencyNames: ['is-subdir'],
        manifest,
        mutation: 'unlinkSome',
        rootDir: process.cwd(),
      },
    ],
    opts
  )

  expect(typeof project.requireModule('is-subdir')).toBe('function')
  expect((await isInnerLink('node_modules', 'is-positive')).isInner).toBeFalsy()
})

test("don't update package when unlinking", async () => {
  const project = prepareEmpty()

  await addDistTag({ package: 'foo', version: '100.0.0', distTag: 'latest' })
  const opts = await testDefaults({ dir: process.cwd() })
  let manifest = await addDependenciesToPackage({}, ['foo'], opts)

  process.chdir('..')

  await writeJsonFile('foo/package.json', {
    name: 'foo',
    version: '100.0.0',
  })

  manifest = await link(['foo'], path.join('project', 'node_modules'), { ...opts, manifest })
  await addDistTag({ package: 'foo', version: '100.1.0', distTag: 'latest' })

  process.chdir('project')
  await mutateModules(
    [
      {
        dependencyNames: ['foo'],
        manifest,
        mutation: 'unlinkSome',
        rootDir: process.cwd(),
      },
    ],
    opts
  )

  expect(project.requireModule('foo/package.json').version).toBe('100.0.0')
})

test(`don't update package when unlinking. Initial link is done on a package w/o ${WANTED_LOCKFILE}`, async () => {
  const project = prepareEmpty()

  const opts = await testDefaults({ dir: process.cwd() })
  process.chdir('..')

  await writeJsonFile('foo/package.json', {
    name: 'foo',
    version: '100.0.0',
  })

  const manifest = await link(['foo'], path.join('project', 'node_modules'), {
    ...opts,
    manifest: {
      dependencies: {
        foo: '^100.0.0',
      },
    },
  })
  await addDistTag({ package: 'foo', version: '100.1.0', distTag: 'latest' })

  process.chdir('project')
  const unlinkResult = await mutateModules(
    [
      {
        dependencyNames: ['foo'],
        manifest,
        mutation: 'unlinkSome',
        rootDir: process.cwd(),
      },
    ],
    opts
  )

  expect(project.requireModule('foo/package.json').version).toBe('100.1.0')
  expect(unlinkResult[0].manifest.dependencies).toStrictEqual({ foo: '^100.0.0' })
})

test('unlink 2 packages. One of them exists in package.json', async () => {
  const project = prepareEmpty()
  const opts = await testDefaults({ fastUnpack: false, dir: process.cwd() })
  process.chdir('..')

  await Promise.all([
    writeJsonFile('is-subdir/package.json', {
      dependencies: {
        'is-windows': '^1.0.0',
      },
      name: 'is-subdir',
      version: '1.0.0',
    }),
    writeJsonFile('is-positive/package.json', {
      name: 'is-positive',
      version: '1.0.0',
    }),
  ])

  const manifest = await link(['is-subdir', 'is-positive'], path.join('project', 'node_modules'), {
    ...opts,
    manifest: {
      dependencies: {
        'is-subdir': '^1.0.0',
      },
    },
  })

  process.chdir('project')
  await mutateModules(
    [
      {
        dependencyNames: ['is-subdir', 'is-positive'],
        manifest,
        mutation: 'unlinkSome',
        rootDir: process.cwd(),
      },
    ],
    opts
  )

  expect(typeof project.requireModule('is-subdir')).toBe('function')
  expect(await exists(path.join('node_modules', 'is-positive'))).toBeFalsy()
})

test('unlink all packages', async () => {
  const project = prepareEmpty()
  const opts = await testDefaults({ fastUnpack: false, dir: process.cwd() })
  process.chdir('..')

  await Promise.all([
    writeJsonFile('is-subdir/package.json', {
      dependencies: {
        'is-windows': '^1.0.0',
      },
      name: 'is-subdir',
      version: '1.0.0',
    }),
    writeJsonFile('logger/package.json', {
      name: '@zkochan/logger',
      version: '0.1.0',
    }),
  ])

  const manifest = await link(['is-subdir', 'logger'], path.join('project', 'node_modules'), {
    ...opts,
    manifest: {
      dependencies: {
        '@zkochan/logger': '^0.1.0',
        'is-subdir': '^1.0.0',
      },
    },
  })

  await mutateModules(
    [
      {
        manifest,
        mutation: 'unlink',
        rootDir: path.resolve('project'),
      },
    ],
    opts
  )

  expect(typeof project.requireModule('is-subdir')).toBe('function')
  expect(typeof project.requireModule('@zkochan/logger')).toBe('object')
})

test("don't warn about scoped packages when running unlink w/o params", async () => {
  prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['@zkochan/logger'], await testDefaults())

  const reporter = sinon.spy()
  await mutateModules(
    [
      {
        manifest,
        mutation: 'unlink',
        rootDir: process.cwd(),
      },
    ],
    await testDefaults({ reporter })
  )

  expect(reporter.calledWithMatch({
    level: 'warn',
    message: '@zkochan/logger is not an external link',
  })).toBeFalsy()
})

test("don't unlink package that is not a link", async () => {
  prepareEmpty()

  const reporter = sinon.spy()

  const manifest = await addDependenciesToPackage({}, ['is-positive'], await testDefaults())

  await mutateModules(
    [
      {
        dependencyNames: ['is-positive'],
        manifest,
        mutation: 'unlinkSome',
        rootDir: process.cwd(),
      },
    ],
    await testDefaults({ reporter })
  )

  expect(reporter.calledWithMatch({
    level: 'warn',
    message: 'is-positive is not an external link',
  })).toBeTruthy()
})

test('unlink would remove global bin', async () => {
  prepareEmpty()
  process.chdir('..')
  fs.mkdirSync('bin')
  fs.mkdirSync('is-subdir')
  fs.writeFileSync('is-subdir/index.js', ' ')

  await Promise.all([
    writeJsonFile('is-subdir/package.json', {
      bin: 'index.js',
      dependencies: {
        'is-windows': '^1.0.0',
      },
      name: 'is-subdir',
      version: '1.0.0',
    }),
  ])

  const opts = await testDefaults({
    fastUnpack: false,
    globalBin: path.resolve('bin'),
    linkToBin: path.resolve('bin'),
    store: path.resolve('.store'),
  })

  const manifest = await link(
    ['is-subdir'],
    path.join('project', 'node_modules'),
    {
      ...opts,
      dir: path.resolve('project'),
      manifest: {
        dependencies: {
          'is-subdir': '^1.0.0',
        },
        name: 'is-subdir',
      },
    }
  )
  expect(fs.existsSync(path.resolve('bin/is-subdir'))).toBeTruthy()

  await mutateModules(
    [
      {
        dependencyNames: ['is-subdir'],
        manifest,
        mutation: 'unlinkSome',
        rootDir: process.cwd(),
      },
    ],
    opts
  )

  expect(fs.existsSync(path.resolve('bin/is-subdir'))).toBeFalsy()
})

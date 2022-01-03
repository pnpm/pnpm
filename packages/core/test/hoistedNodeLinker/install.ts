import fs from 'fs'
import path from 'path'
import { addDependenciesToPackage, install, mutateModules } from '@pnpm/core'
import { prepareEmpty } from '@pnpm/prepare'
import { sync as loadJsonFile } from 'load-json-file'
import { addDistTag, testDefaults } from '../utils'

test('installing with hoisted node-linker', async () => {
  prepareEmpty()

  await install({
    dependencies: {
      send: '0.17.2',
      'has-flag': '1.0.0',
      ms: '1.0.0',
    },
  }, await testDefaults({
    nodeLinker: 'hoisted',
  }))

  expect(fs.realpathSync('node_modules/send')).toEqual(path.resolve('node_modules/send'))
  expect(fs.realpathSync('node_modules/has-flag')).toEqual(path.resolve('node_modules/has-flag'))
  expect(fs.realpathSync('node_modules/ms')).toEqual(path.resolve('node_modules/ms'))
  expect(fs.existsSync('node_modules/send/node_modules/ms')).toBeTruthy()
})

test('overwriting (is-positive@3.0.0 with is-positive@latest)', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage(
    {},
    ['is-positive@3.0.0'],
    await testDefaults({ nodeLinker: 'hoisted', save: true })
  )

  await project.storeHas('is-positive', '3.0.0')

  const updatedManifest = await addDependenciesToPackage(
    manifest,
    ['is-positive@latest'],
    await testDefaults({ nodeLinker: 'hoisted', save: true })
  )

  await project.storeHas('is-positive', '3.1.0')
  expect(updatedManifest.dependencies?.['is-positive']).toBe('3.1.0')
  expect(loadJsonFile<{ version: string }>('node_modules/is-positive/package.json').version).toBe('3.1.0')
})

test('preserve subdeps on update', async () => {
  prepareEmpty()

  await addDistTag('foobarqar', '1.0.0', 'latest')

  const manifest = await addDependenciesToPackage(
    {},
    ['foobarqar@1.0.0', 'bar@100.1.0'],
    await testDefaults({ nodeLinker: 'hoisted' })
  )

  await addDependenciesToPackage(
    manifest,
    ['foobarqar@1.0.1'],
    await testDefaults({ nodeLinker: 'hoisted' })
  )

  expect(loadJsonFile<{ version: string }>('node_modules/bar/package.json').version).toBe('100.1.0')
  expect(loadJsonFile<{ version: string }>('node_modules/foobarqar/package.json').version).toBe('1.0.1')
  expect(loadJsonFile<{ version: string }>('node_modules/foobarqar/node_modules/bar/package.json').version).toBe('100.0.0')
})

test('adding a new dependency to one of the the workspace projects', async () => {
  prepareEmpty()

  let [{ manifest }] = await mutateModules([
    {
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          bar: '100.0.0',
        },
      },
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 1,
      manifest: {
        name: 'project-2',
        version: '1.0.0',

        dependencies: {
          foobarqar: '1.0.0',
        },
      },
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ], await testDefaults({ nodeLinker: 'hoisted' }))
  manifest = await addDependenciesToPackage(
    manifest,
    ['is-negative@1.0.0'],
    await testDefaults({ nodeLinker: 'hoisted', prefix: path.resolve('project-1'), targetDependenciesField: 'devDependencies' })
  )

  expect(manifest.dependencies).toStrictEqual({ bar: '100.0.0' })
  expect(manifest.devDependencies).toStrictEqual({ 'is-negative': '1.0.0' })
  expect(loadJsonFile<{ version: string }>('node_modules/bar/package.json').version).toBe('100.0.0')
  expect(loadJsonFile<{ version: string }>('node_modules/is-negative/package.json').version).toBe('1.0.0')
})

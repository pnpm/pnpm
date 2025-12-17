import fs from 'fs'
import { createExportableManifest, type MakePublishManifestOptions } from '@pnpm/exportable-manifest'
import { requireHooks } from '@pnpm/pnpmfile'
import { prepare } from '@pnpm/prepare'
import { sync as writeYamlFile } from 'write-yaml-file'

const defaultOpts: MakePublishManifestOptions = {
  catalogs: {},
}

test('readPackageForPublishing basic hook', async () => {
  prepare()

  fs.writeFileSync('.pnpmfile.cjs', `
module.exports = {
  hooks: {
    readPackageForPublishing: (pkg, dir, context) => {
      context.log(dir)
      pkg.foo = 'bar'
      return pkg // return optional
    },
  },
}`, 'utf8')

  const { hooks } = await requireHooks(process.cwd(), { tryLoadDefaultPnpmfile: true })
  expect(await createExportableManifest(process.cwd(), {
    name: 'foo',
    version: '1.0.0',
    dependencies: {
      qar: '2',
    },
  }, { ...defaultOpts, hooks })).toStrictEqual({
    name: 'foo',
    version: '1.0.0',
    dependencies: {
      qar: '2',
    },
    foo: 'bar',
  })
})

test('readPackageForPublishing hook returns new manifest', async () => {
  prepare()

  fs.writeFileSync('.pnpmfile.cjs', `
module.exports = {
  hooks: {
    readPackageForPublishing: (pkg) => {
      return { type: 'module' }
    },
  },
}`, 'utf8')

  const { hooks } = await requireHooks(process.cwd(), { tryLoadDefaultPnpmfile: true })
  expect(await createExportableManifest(process.cwd(), {
    name: 'foo',
    version: '1.0.0',
  }, { ...defaultOpts, hooks })).toStrictEqual({
    type: 'module',
  })
})

test('readPackageForPublishing hook in multiple pnpmfiles', async () => {
  prepare()

  fs.writeFileSync('pnpmfile1.cjs', `
module.exports = {
  hooks: {
    readPackageForPublishing: (pkg) => {
      pkg.foo = 'foo'
    },
  },
}`, 'utf8')
  fs.writeFileSync('pnpmfile2.cjs', `
module.exports = {
  hooks: {
    readPackageForPublishing: (pkg) => {
      pkg.bar = 'bar'
    },
  },
}`, 'utf8')
  const pnpmfiles = ['pnpmfile1.cjs', 'pnpmfile2.cjs']
  writeYamlFile('pnpm-workspace.yaml', { pnpmfile: pnpmfiles })

  const { hooks } = await requireHooks(process.cwd(), { pnpmfiles })
  expect(await createExportableManifest(process.cwd(), {
    name: 'foo',
    version: '1.0.0',
    dependencies: {
      qar: '2',
    },
  }, { ...defaultOpts, hooks })).toStrictEqual({
    name: 'foo',
    version: '1.0.0',
    dependencies: {
      qar: '2',
    },
    foo: 'foo',
    bar: 'bar',
  })
})

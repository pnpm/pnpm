import fs from 'fs'
import { createExportableManifest, type MakePublishManifestOptions } from '@pnpm/exportable-manifest'
import { requireHooks } from '@pnpm/pnpmfile'
import { prepare } from '@pnpm/prepare'
import { sync as writeYamlFile } from 'write-yaml-file'

const defaultOpts: MakePublishManifestOptions = {
  catalogs: {},
}

test('basic test', async () => {
  prepare()

  fs.writeFileSync('.pnpmfile.cjs', `
module.exports = {
  hooks: {
    beforePacking: (pkg, dir, context) => {
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

test('hook returns new manifest', async () => {
  prepare()

  fs.writeFileSync('.pnpmfile.cjs', `
module.exports = {
  hooks: {
    beforePacking: (pkg) => {
      return { type: 'module', ...pkg }
    },
  },
}`, 'utf8')

  const { hooks } = await requireHooks(process.cwd(), { tryLoadDefaultPnpmfile: true })
  expect(await createExportableManifest(process.cwd(), {
    name: 'foo',
    version: '1.0.0',
  }, { ...defaultOpts, hooks })).toStrictEqual({
    type: 'module',
    name: 'foo',
    version: '1.0.0',
  })
})

test('hook in multiple pnpmfiles', async () => {
  prepare()

  const pnpmfiles = ['pnpmfile1.cjs', 'pnpmfile2.cjs']
  fs.writeFileSync(pnpmfiles[0], `
module.exports = {
  hooks: {
    beforePacking: (pkg) => {
      pkg.foo = 'foo'
    },
  },
}`, 'utf8')
  fs.writeFileSync(pnpmfiles[1], `
module.exports = {
  hooks: {
    beforePacking: (pkg) => {
      pkg.bar = 'bar'
    },
  },
}`, 'utf8')
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

import fs from 'fs'
import { createExportableManifest, type MakePublishManifestOptions } from '@pnpm/exportable-manifest'
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

  expect(await createExportableManifest(process.cwd(), {
    name: 'foo',
    version: '1.0.0',
    dependencies: {
      qar: '2',
    },
  }, defaultOpts)).toStrictEqual({
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

  expect(await createExportableManifest(process.cwd(), {
    name: 'foo',
    version: '1.0.0',
  }, defaultOpts)).toStrictEqual({
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
  writeYamlFile('pnpm-workspace.yaml', { pnpmfile: ['pnpmfile1.cjs', 'pnpmfile2.cjs'] })

  expect(await createExportableManifest(process.cwd(), {
    name: 'foo',
    version: '1.0.0',
    dependencies: {
      qar: '2',
    },
  }, defaultOpts)).toStrictEqual({
    name: 'foo',
    version: '1.0.0',
    dependencies: {
      qar: '2',
    },
    foo: 'foo',
    bar: 'bar',
  })
})

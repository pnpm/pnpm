import { promises as fs } from 'fs'
import path from 'path'
import { PackageManifest } from '@pnpm/types'
import prepare, { preparePackages } from '@pnpm/prepare'
import loadJsonFile from 'load-json-file'
import writeYamlFile from 'write-yaml-file'
import { execPnpm } from './utils'

test('readPackage hook in single project doesn\'t modify manifest', async () => {
  const project = prepare()
  const pnpmfile = `
      module.exports = { hooks: { readPackage } }
      function readPackage (pkg, context) {
        if (pkg.name === 'project') {
          context.log('good')
          pkg.dependencies = pkg.dependencies || {}
          pkg.dependencies['dep-of-pkg-with-1-dep'] = '100.1.0'
        }
      return pkg
      }
  `
  await fs.writeFile('.pnpmfile.cjs', pnpmfile, 'utf8')
  await execPnpm(['add', 'is-positive@1.0.0'])
  let pkg: PackageManifest = await loadJsonFile(path.resolve('package.json'))
  expect(pkg?.dependencies).toStrictEqual({ 'is-positive': '1.0.0' }) // add dependency & readPackage hook work

  await execPnpm(['update', 'is-positive@2.0.0'])
  pkg = await loadJsonFile(path.resolve('package.json'))
  expect(pkg?.dependencies).toStrictEqual({ 'is-positive': '2.0.0' }) // update dependency & readPackage hook work

  await execPnpm(['install'])
  pkg = await loadJsonFile(path.resolve('package.json'))
  expect(pkg?.dependencies).toStrictEqual({ 'is-positive': '2.0.0' }) // install & readPackage hook work

  await execPnpm(['remove', 'is-positive'])
  pkg = await loadJsonFile(path.resolve('package.json'))
  expect(pkg.dependencies).toBeFalsy() // remove & readPackage hook work
  await project.hasNot('is-positive')
})

test('readPackage hook in monorepo doesn\'t modify manifest', async () => {
  preparePackages([
    {
      name: 'project-a',
      version: '1.0.0',
    },
    {
      name: 'project-b',
      version: '1.0.0',
    },
  ])

  const pnpmfile = `
      module.exports = { hooks: { readPackage } }
      function readPackage (pkg) {
        if (pkg.name === 'project-a') {
          pkg.dependencies = pkg.dependencies || {}
          pkg.dependencies['dep-of-pkg-with-1-dep'] = '100.1.0'
        }
        return pkg
      }
    `
  await fs.writeFile('.pnpmfile.cjs', pnpmfile, 'utf8')
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['add', 'is-positive@1.0.0', '--filter', 'project-a'])
  let pkg: PackageManifest = await loadJsonFile(path.resolve('project-a/package.json'))
  expect(pkg?.dependencies).toStrictEqual({ 'is-positive': '1.0.0' }) // add dependency & readPackage hook work

  await execPnpm(['update', 'is-positive@2.0.0', '--filter', 'project-a'])
  pkg = await loadJsonFile(path.resolve('project-a/package.json'))
  expect(pkg?.dependencies).toStrictEqual({ 'is-positive': '2.0.0' }) // update dependency & readPackage hook work

  await execPnpm(['install', '--filter', 'project-a'])
  pkg = await loadJsonFile(path.resolve('project-a/package.json'))
  expect(pkg?.dependencies).toStrictEqual({ 'is-positive': '2.0.0' }) // install & readPackage hook work

  await execPnpm(['remove', 'is-positive', '--filter', 'project-a'])
  pkg = await loadJsonFile(path.resolve('project-a/package.json'))
  expect(pkg.dependencies).toBeFalsy() // remove & readPackage hook work
})

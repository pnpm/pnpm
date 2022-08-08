import { promises as fs } from 'fs'
import path from 'path'
import { PackageManifest } from '@pnpm/types'
import prepare, { preparePackages } from '@pnpm/prepare'
import loadJsonFile from 'load-json-file'
import writeYamlFile from 'write-yaml-file'
import { execPnpm, execPnpmSync } from './utils'

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

test('filterLog hook filters peer dependency warning', async () => {
  prepare()
  const pnpmfile = `
      module.exports = { hooks: { filterLog } }
      function filterLog (log) {
        if (/requires a peer of rollup/.test(log.message)) {
          return false
        }
        return true
      }
    `
  await fs.writeFile('.pnpmfile.cjs', pnpmfile, 'utf8')
  const result = execPnpmSync(['add', '@rollup/pluginutils@3.1.0', '--no-strict-peer-dependencies'])

  expect(result.status).toBe(0)
  expect(result.stdout.toString()).toEqual(
    expect.not.stringContaining('requires a peer of rollup')
  )
})

test('importPackage hooks', async () => {
  prepare()
  const pnpmfile = `
    const fs = require('fs')

    module.exports = { hooks: { importPackage } }

    function importPackage (to, opts) {
      fs.writeFileSync('args.json', JSON.stringify([to, opts]), 'utf8')
      return {}
    }
  `

  const npmrc = `
    global-pnpmfile=.pnpmfile.cjs
  `

  await fs.writeFile('.npmrc', npmrc, 'utf8')
  await fs.writeFile('.pnpmfile.cjs', pnpmfile, 'utf8')

  await execPnpm(['add', 'is-positive@1.0.0'])

  const [to, opts] = await loadJsonFile<any>('args.json') // eslint-disable-line

  expect(typeof to).toBe('string')
  expect(Object.keys(opts.filesMap).sort()).toStrictEqual([
    'index.js',
    'license',
    'package.json',
    'readme.md',
  ])
})

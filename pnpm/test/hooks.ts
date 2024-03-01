import fs from 'fs'
import path from 'path'
import { createBase32Hash } from '@pnpm/crypto.base32-hash'
import { type PackageManifest } from '@pnpm/types'
import { prepare, preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import loadJsonFile from 'load-json-file'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpm, execPnpmSync } from './utils'

test('readPackage hook in single project doesn\'t modify manifest', async () => {
  const project = prepare()
  const pnpmfile = `
      module.exports = { hooks: { readPackage } }
      function readPackage (pkg, context) {
        if (pkg.name === 'project') {
          context.log('good')
          pkg.dependencies = pkg.dependencies || {}
          pkg.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '100.1.0'
        }
      return pkg
      }
  `
  fs.writeFileSync('.pnpmfile.cjs', pnpmfile, 'utf8')
  await execPnpm(['add', 'is-positive@1.0.0'])
  let pkg: PackageManifest = loadJsonFile.sync(path.resolve('package.json'))
  expect(pkg?.dependencies).toStrictEqual({ 'is-positive': '1.0.0' }) // add dependency & readPackage hook work

  await execPnpm(['update', 'is-positive@2.0.0'])
  pkg = loadJsonFile.sync(path.resolve('package.json'))
  expect(pkg?.dependencies).toStrictEqual({ 'is-positive': '2.0.0' }) // update dependency & readPackage hook work

  await execPnpm(['install'])
  pkg = loadJsonFile.sync(path.resolve('package.json'))
  expect(pkg?.dependencies).toStrictEqual({ 'is-positive': '2.0.0' }) // install & readPackage hook work

  await execPnpm(['remove', 'is-positive'])
  pkg = loadJsonFile.sync(path.resolve('package.json'))
  expect(pkg.dependencies).toBeFalsy() // remove & readPackage hook work
  project.hasNot('is-positive')

  // Reset for --lockfile-only checks
  fs.unlinkSync('pnpm-lock.yaml')

  await execPnpm(['install', '--lockfile-only'])
  pkg = loadJsonFile.sync(path.resolve('package.json'))
  expect(pkg.dependencies).toBeFalsy() // install --lockfile-only & readPackage hook work, without pnpm-lock.yaml

  // runs with pnpm-lock.yaml should not mutate local projects
  await execPnpm(['install', '--lockfile-only'])
  pkg = loadJsonFile.sync(path.resolve('package.json'))
  expect(pkg.dependencies).toBeFalsy() // install --lockfile-only & readPackage hook work, with pnpm-lock.yaml
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
          pkg.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '100.1.0'
        }
        return pkg
      }
    `
  fs.writeFileSync('.pnpmfile.cjs', pnpmfile, 'utf8')
  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['add', 'is-positive@1.0.0', '--filter', 'project-a'])
  let pkg: PackageManifest = loadJsonFile.sync(path.resolve('project-a/package.json'))
  expect(pkg?.dependencies).toStrictEqual({ 'is-positive': '1.0.0' }) // add dependency & readPackage hook work

  await execPnpm(['update', 'is-positive@2.0.0', '--filter', 'project-a'])
  pkg = loadJsonFile.sync(path.resolve('project-a/package.json'))
  expect(pkg?.dependencies).toStrictEqual({ 'is-positive': '2.0.0' }) // update dependency & readPackage hook work

  await execPnpm(['install', '--filter', 'project-a'])
  pkg = loadJsonFile.sync(path.resolve('project-a/package.json'))
  expect(pkg?.dependencies).toStrictEqual({ 'is-positive': '2.0.0' }) // install & readPackage hook work

  await execPnpm(['remove', 'is-positive', '--filter', 'project-a'])
  pkg = loadJsonFile.sync(path.resolve('project-a/package.json'))
  expect(pkg.dependencies).toBeFalsy() // remove & readPackage hook work

  // Reset for --lockfile-only checks
  fs.unlinkSync('pnpm-lock.yaml')

  await execPnpm(['install', '--lockfile-only'])
  pkg = loadJsonFile.sync(path.resolve('project-a/package.json'))
  expect(pkg.dependencies).toBeFalsy() // install --lockfile-only & readPackage hook work, without pnpm-lock.yaml

  // runs with pnpm-lock.yaml should not mutate local projects
  await execPnpm(['install', '--lockfile-only'])
  pkg = loadJsonFile.sync(path.resolve('project-a/package.json'))
  expect(pkg.dependencies).toBeFalsy() // install --lockfile-only & readPackage hook work, with pnpm-lock.yaml
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
  fs.writeFileSync('.pnpmfile.cjs', pnpmfile, 'utf8')
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

  fs.writeFileSync('.npmrc', npmrc, 'utf8')
  fs.writeFileSync('.pnpmfile.cjs', pnpmfile, 'utf8')

  await execPnpm(['add', 'is-positive@1.0.0'])

  const [to, opts] = loadJsonFile.sync<any>('args.json') // eslint-disable-line

  expect(typeof to).toBe('string')
  expect(Object.keys(opts.filesMap).sort()).toStrictEqual([
    'index.js',
    'license',
    'package.json',
    'readme.md',
  ])
})

test('should use default fetchers if no custom fetchers are defined', async () => {
  const project = prepare()

  const pnpmfile = `
    const fs = require('fs')

    module.exports = {
      hooks: {
        fetchers: {}
      }
    }
  `

  const npmrc = `
    global-pnpmfile=.pnpmfile.cjs
  `

  fs.writeFileSync('.npmrc', npmrc, 'utf8')
  fs.writeFileSync('.pnpmfile.cjs', pnpmfile, 'utf8')

  await execPnpm(['add', 'is-positive@1.0.0'])

  project.cafsHas('is-positive', '1.0.0')
})

test('custom fetcher can call default fetcher', async () => {
  const project = prepare()

  const pnpmfile = `
    const fs = require('fs')

    module.exports = {
      hooks: {
        fetchers: {
          remoteTarball: ({ defaultFetchers }) => {
            return (cafs, resolution, opts) => {
              fs.writeFileSync('args.json', JSON.stringify({ resolution, opts }), 'utf8')
              return defaultFetchers.remoteTarball(cafs, resolution, opts)
            }
          }
        }
      }
    }
  `

  const npmrc = `
    global-pnpmfile=.pnpmfile.cjs
  `

  fs.writeFileSync('.npmrc', npmrc, 'utf8')
  fs.writeFileSync('.pnpmfile.cjs', pnpmfile, 'utf8')

  await execPnpm(['add', 'is-positive@1.0.0'])

  project.cafsHas('is-positive', '1.0.0')

  const args = loadJsonFile.sync<any>('args.json') // eslint-disable-line

  expect(args.resolution).toEqual({
    integrity: 'sha512-xxzPGZ4P2uN6rROUa5N9Z7zTX6ERuE0hs6GUOc/cKBLF2NqKc16UwqHMt3tFg4CO6EBTE5UecUasg+3jZx3Ckg==',
    tarball: `http://localhost:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-1.0.0.tgz`,
  })

  expect(args.opts).toBeDefined()
})

test('adding or changing pnpmfile should change pnpmfileChecksum and module structure', async () => {
  const project = prepare({
    dependencies: {
      '@pnpm.e2e/pkg-with-good-optional': '1.0.0',
    },
  })

  await execPnpm(['install'])

  const lockfile0 = project.readLockfile()
  expect(lockfile0.pnpmfileChecksum).toBeUndefined()
  expect(lockfile0.packages).toHaveProperty(['/@pnpm.e2e/pkg-with-good-optional@1.0.0'])
  expect(lockfile0.packages).toHaveProperty(['/is-positive@1.0.0'])

  const pnpmfile1 = `
    function readPackage (pkg) {
      if (pkg.optionalDependencies) {
        delete pkg.optionalDependencies
      }
      return pkg
    }

    module.exports.hooks = { readPackage }
  `
  fs.writeFileSync('.pnpmfile.cjs', pnpmfile1)
  await execPnpm(['install'])

  const lockfile1 = project.readLockfile()
  expect(lockfile1.pnpmfileChecksum).toBe(createBase32Hash(pnpmfile1))
  expect(lockfile1.packages).toHaveProperty(['/@pnpm.e2e/pkg-with-good-optional@1.0.0'])
  expect(lockfile1.packages).not.toHaveProperty(['/is-positive@1.0.0']) // this should be removed due to being optional dependency

  const pnpmfile2 = `
    function readPackage (pkg) {
      if (pkg.name === '@pnpm.e2e/pkg-with-good-optional') {
        pkg.dependencies['@pnpm.e2e/foo'] = '100.0.0'
      }
      if (pkg.name === 'is-positive') {
        pkg.dependencies['@pnpm.e2e/bar'] = '100.0.0'
      }
      return pkg
    }

    module.exports.hooks = { readPackage }
  `
  fs.writeFileSync('.pnpmfile.cjs', pnpmfile2)
  await execPnpm(['install'])

  const lockfile2 = project.readLockfile()
  expect(lockfile2.pnpmfileChecksum).toBe(createBase32Hash(pnpmfile2))
  expect(lockfile2.packages).toMatchObject({
    '/@pnpm.e2e/foo@100.0.0': expect.any(Object),
    '/@pnpm.e2e/bar@100.0.0': expect.any(Object),
    '/@pnpm.e2e/pkg-with-good-optional@1.0.0': {
      dependencies: {
        '@pnpm.e2e/foo': '100.0.0',
      },
    },
    '/is-positive@1.0.0': {
      dependencies: {
        '@pnpm.e2e/bar': '100.0.0',
      },
    },
  })

  fs.unlinkSync('.pnpmfile.cjs')
  await execPnpm(['install'])

  const lockfile3 = project.readLockfile()
  expect(lockfile3).toStrictEqual(lockfile0)
})

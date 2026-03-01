import fs from 'fs'
import path from 'path'
import { createHash } from '@pnpm/crypto.hash'
import { type PackageManifest } from '@pnpm/types'
import { prepare, preparePackages } from '@pnpm/prepare'
import { getIntegrity } from '@pnpm/registry-mock'
import { loadJsonFileSync } from 'load-json-file'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpm, execPnpmSync } from './utils/index.js'

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
  let pkg: PackageManifest = loadJsonFileSync(path.resolve('package.json'))
  expect(pkg?.dependencies).toStrictEqual({ 'is-positive': '1.0.0' }) // add dependency & readPackage hook work

  await execPnpm(['update', 'is-positive@2.0.0'])
  pkg = loadJsonFileSync(path.resolve('package.json'))
  expect(pkg?.dependencies).toStrictEqual({ 'is-positive': '2.0.0' }) // update dependency & readPackage hook work

  await execPnpm(['install'])
  pkg = loadJsonFileSync(path.resolve('package.json'))
  expect(pkg?.dependencies).toStrictEqual({ 'is-positive': '2.0.0' }) // install & readPackage hook work

  await execPnpm(['remove', 'is-positive'])
  pkg = loadJsonFileSync(path.resolve('package.json'))
  expect(pkg.dependencies).toBeFalsy() // remove & readPackage hook work
  project.hasNot('is-positive')

  // Reset for --lockfile-only checks
  fs.unlinkSync('pnpm-lock.yaml')

  await execPnpm(['install', '--lockfile-only'])
  pkg = loadJsonFileSync(path.resolve('package.json'))
  expect(pkg.dependencies).toBeFalsy() // install --lockfile-only & readPackage hook work, without pnpm-lock.yaml

  // runs with pnpm-lock.yaml should not mutate local projects
  await execPnpm(['install', '--lockfile-only'])
  pkg = loadJsonFileSync(path.resolve('package.json'))
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
  let pkg: PackageManifest = loadJsonFileSync(path.resolve('project-a/package.json'))
  expect(pkg?.dependencies).toStrictEqual({ 'is-positive': '1.0.0' }) // add dependency & readPackage hook work

  await execPnpm(['update', 'is-positive@2.0.0', '--filter', 'project-a'])
  pkg = loadJsonFileSync(path.resolve('project-a/package.json'))
  expect(pkg?.dependencies).toStrictEqual({ 'is-positive': '2.0.0' }) // update dependency & readPackage hook work

  await execPnpm(['install', '--filter', 'project-a'])
  pkg = loadJsonFileSync(path.resolve('project-a/package.json'))
  expect(pkg?.dependencies).toStrictEqual({ 'is-positive': '2.0.0' }) // install & readPackage hook work

  await execPnpm(['remove', 'is-positive', '--filter', 'project-a'])
  pkg = loadJsonFileSync(path.resolve('project-a/package.json'))
  expect(pkg.dependencies).toBeFalsy() // remove & readPackage hook work

  // Reset for --lockfile-only checks
  fs.unlinkSync('pnpm-lock.yaml')

  await execPnpm(['install', '--lockfile-only'])
  pkg = loadJsonFileSync(path.resolve('project-a/package.json'))
  expect(pkg.dependencies).toBeFalsy() // install --lockfile-only & readPackage hook work, without pnpm-lock.yaml

  // runs with pnpm-lock.yaml should not mutate local projects
  await execPnpm(['install', '--lockfile-only'])
  pkg = loadJsonFileSync(path.resolve('project-a/package.json'))
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
      fs.writeFileSync('args.json', JSON.stringify([to, Array.from(opts.filesMap.keys()).sort()]), 'utf8')
      return {}
    }
  `

  writeYamlFile('pnpm-workspace.yaml', {
    globalPnpmfile: '.pnpmfile.cjs',
  })

  fs.writeFileSync('.pnpmfile.cjs', pnpmfile, 'utf8')

  await execPnpm(['add', 'is-positive@1.0.0'])

  const [to, files] = loadJsonFileSync<any>('args.json') // eslint-disable-line

  expect(typeof to).toBe('string')
  expect(files).toStrictEqual([
    'index.js',
    'license',
    'package.json',
    'readme.md',
  ])
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
  expect(lockfile0.packages).toHaveProperty(['@pnpm.e2e/pkg-with-good-optional@1.0.0'])
  expect(lockfile0.packages).toHaveProperty(['is-positive@1.0.0'])

  const pnpmfile1 = `
    function readPackage (pkg) {
      if (pkg.optionalDependencies) {
        // Also remove optional deps from dependencies since npm duplicates them there
        for (const dep of Object.keys(pkg.optionalDependencies)) {
          delete pkg.dependencies?.[dep]
        }
        delete pkg.optionalDependencies
      }
      return pkg
    }

    module.exports.hooks = { readPackage }
  `
  fs.writeFileSync('.pnpmfile.cjs', pnpmfile1)
  await execPnpm(['install'])

  const lockfile1 = project.readLockfile()
  expect(lockfile1.pnpmfileChecksum).toBe(createHash(pnpmfile1))
  expect(lockfile1.packages).toHaveProperty(['@pnpm.e2e/pkg-with-good-optional@1.0.0'])
  expect(lockfile1.packages).not.toHaveProperty(['is-positive@1.0.0']) // this should be removed due to being optional dependency

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
  expect(lockfile2.pnpmfileChecksum).toBe(createHash(pnpmfile2))
  expect(lockfile2.snapshots).toMatchObject({
    '@pnpm.e2e/foo@100.0.0': expect.any(Object),
    '@pnpm.e2e/bar@100.0.0': expect.any(Object),
    '@pnpm.e2e/pkg-with-good-optional@1.0.0': {
      dependencies: {
        '@pnpm.e2e/foo': '100.0.0',
      },
    },
    'is-positive@1.0.0': {
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

test('loading a pnpmfile from a config dependency', async () => {
  prepare({
    dependencies: {
      '@pnpm/x': '1.0.0',
    },
  })

  writeYamlFile('pnpm-workspace.yaml', {
    configDependencies: {
      '@pnpm.e2e/exports-pnpmfile': `1.0.0+${getIntegrity('@pnpm.e2e/exports-pnpmfile', '1.0.0')}`,
    },
  })

  await execPnpm(['install', '--config.pnpmfile=node_modules/.pnpm-config/@pnpm.e2e/exports-pnpmfile/pnpmfile.cjs'])

  expect(fs.readdirSync('node_modules/.pnpm')).toContain('@pnpm+y@1.0.0')
})

test('updateConfig hook', async () => {
  prepare()
  const pnpmfile = `
module.exports = {
  hooks: {
    updateConfig: (config) => ({
      ...config,
      nodeLinker: 'hoisted',
    }),
  },
}`

  fs.writeFileSync('.pnpmfile.cjs', pnpmfile, 'utf8')

  await execPnpm(['add', 'is-odd@1.0.0'])

  const nodeModulesFiles = fs.readdirSync('node_modules')
  expect(nodeModulesFiles).toContain('kind-of')
  expect(nodeModulesFiles).toContain('is-number')
})

test('loading an ESM pnpmfile', async () => {
  prepare()

  fs.writeFileSync('.pnpmfile.mjs', `
export const hooks = {
  updateConfig: (config) => ({
    ...config,
    nodeLinker: 'hoisted',
  }),
}`, 'utf8')
  writeYamlFile('pnpm-workspace.yaml', { pnpmfile: ['.pnpmfile.mjs'] })

  await execPnpm(['add', 'is-odd@1.0.0'])

  const nodeModulesFiles = fs.readdirSync('node_modules')
  expect(nodeModulesFiles).toContain('kind-of')
  expect(nodeModulesFiles).toContain('is-number')
})

test('loading multiple pnpmfiles', async () => {
  prepare()

  fs.writeFileSync('pnpmfile1.cjs', `
module.exports = {
  hooks: {
    updateConfig: (config) => ({
      ...config,
      nodeLinker: 'hoisted',
    }),
  },
}`, 'utf8')
  fs.writeFileSync('pnpmfile2.cjs', `
module.exports = {
  hooks: {
    readPackage: (pkg) => {
      if (pkg.name === 'is-odd') {
        pkg.dependencies['is-even'] = '1.0.0'
      }
      return pkg
    },
  },
}`, 'utf8')
  writeYamlFile('pnpm-workspace.yaml', { pnpmfile: ['pnpmfile1.cjs', 'pnpmfile2.cjs'] })

  await execPnpm(['add', 'is-odd@1.0.0'])

  const nodeModulesFiles = fs.readdirSync('node_modules')
  expect(nodeModulesFiles).toContain('kind-of')
  expect(nodeModulesFiles).toContain('is-number')
  expect(nodeModulesFiles).toContain('is-even')
})

test('automatically loading pnpmfile from a config dependency that has a name that starts with "@pnpm/plugin-"', async () => {
  prepare()

  await execPnpm(['add', '--config', '@pnpm/plugin-pnpmfile'])
  await execPnpm(['add', 'is-odd@1.0.0'])

  const nodeModulesFiles = fs.readdirSync('node_modules')
  expect(nodeModulesFiles).toContain('kind-of')
  expect(nodeModulesFiles).toContain('is-number')
})

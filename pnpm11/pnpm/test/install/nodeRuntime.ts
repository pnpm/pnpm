import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from '../utils/index.js'

const skipOnWindows = process.platform === 'win32' ? test.skip : test

test('installing a CLI tool that requires a specific version of Node.js to be installed alongside it', async () => {
  prepare()
  fs.writeFileSync('pnpm-workspace.yaml', 'allowBuilds: { "@pnpm.e2e/cli-with-node-engine@1.0.0": true }', 'utf8')

  await execPnpm(['add', '@pnpm.e2e/cli-with-node-engine@1.0.0'])
  await execPnpm(['exec', 'cli-with-node-engine'])
  expect(fs.readFileSync('node-version', 'utf8')).toBe('v22.19.0')
})

skipOnWindows('downloaded runtime is available to dependency lifecycle scripts without a system Node.js', async () => {
  const project = prepare({
    dependencies: {
      '@pnpm.e2e/install-script-example': '1.0.0',
    },
    devEngines: {
      runtime: {
        name: 'node',
        version: '22.19.0',
        onFail: 'download',
      },
    },
  })
  writeYamlFileSync('pnpm-workspace.yaml', {
    allowBuilds: {
      '@pnpm.e2e/install-script-example': true,
    },
    scriptShell: process.platform === 'win32' ? process.env.ComSpec : '/bin/sh',
  })
  const opts = { env: { PATH: '' } }

  await execPnpm(['install', '--ignore-scripts', '--virtual-store-dir=.pnpm'])
  fs.rmSync('node_modules', { force: true, recursive: true })
  fs.rmSync('pnpm-lock.yaml', { force: true })

  await execPnpm(['install', '--prefer-offline', '--virtual-store-dir=.pnpm'], opts)
  project.has('@pnpm.e2e/install-script-example/generated-by-install.js')

  fs.rmSync('node_modules', { force: true, recursive: true })
  await execPnpm(['install', '--frozen-lockfile', '--prefer-offline', '--virtual-store-dir=.pnpm'], opts)
  project.has('@pnpm.e2e/install-script-example/generated-by-install.js')
})

test('a devEngines.runtime is never promoted into a catalog under catalogMode=strict', async () => {
  const project = prepare({
    devEngines: {
      runtime: {
        name: 'node',
        version: '24.0.0',
        onFail: 'download',
      },
    },
  })
  writeYamlFileSync('pnpm-workspace.yaml', { catalogMode: 'strict' })

  await execPnpm(['install'])

  // The runtime resolves and installs as usual.
  project.isExecutable('.bin/node')

  // It is not turned into a catalog entry...
  const workspaceManifest = readYamlFileSync<{ catalog?: unknown, catalogs?: unknown }>('pnpm-workspace.yaml')
  expect(workspaceManifest.catalog).toBeUndefined()
  expect(workspaceManifest.catalogs).toBeUndefined()

  // ...and stays in devEngines.runtime instead of leaking into devDependencies.
  const manifest = JSON.parse(fs.readFileSync('package.json', 'utf8'))
  expect(manifest.devEngines.runtime).toMatchObject({ name: 'node', version: '24.0.0' })
  expect(manifest.devDependencies?.node).toBeUndefined()

  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].devDependencies).toStrictEqual({
    node: {
      specifier: 'runtime:24.0.0',
      version: 'runtime:24.0.0',
    },
  })
})

test('devEngines.runtime range without download does not replace running Node.js version for optional dependencies', async () => {
  const runningNodeMajor = Number(process.versions.node.split('.')[0])
  const project = prepare({
    optionalDependencies: {
      dependency: 'file:dependency',
    },
    devEngines: {
      runtime: {
        name: 'node',
        version: `>=${runningNodeMajor - 1}.0.0`,
        onFail: 'error',
      },
    },
  })
  fs.mkdirSync('dependency')
  fs.writeFileSync('dependency/package.json', JSON.stringify({
    name: 'dependency',
    version: '1.0.0',
    engines: {
      node: `>=${runningNodeMajor}.0.0`,
    },
  }))

  await execPnpm(['install'])

  project.has('dependency')
})

test('optional dependencies are checked against the Node.js version locked for a devEngines.runtime range', async () => {
  const project = prepare({
    optionalDependencies: {
      dependency: 'file:dependency',
    },
    devEngines: {
      runtime: {
        name: 'node',
        version: '^24.0.0',
        onFail: 'download',
      },
    },
  })
  fs.mkdirSync('dependency')
  fs.writeFileSync('dependency/package.json', JSON.stringify({
    name: 'dependency',
    version: '1.0.0',
    engines: {
      node: '>=24.1.0',
    },
  }))

  await execPnpm(['install', '--lockfile-only'])
  await execPnpm(['install', '--frozen-lockfile', '--no-runtime'])

  expect(project.readModulesManifest()?.skipped).toStrictEqual([])
})

test('a resolving install checks optional dependencies against the Node.js version resolved for a devEngines.runtime range', async () => {
  const project = prepare({
    optionalDependencies: {
      '@pnpm.e2e/requires-node-24-1': '1.0.0',
    },
    devEngines: {
      runtime: {
        name: 'node',
        version: '^24.0.0',
        onFail: 'download',
      },
    },
  })

  await execPnpm(['install', '--no-runtime'])

  expect(project.readModulesManifest()?.skipped).toStrictEqual([])
  project.has('@pnpm.e2e/requires-node-24-1')
})

test('pnpm add checks optional dependencies against the Node.js version locked for a devEngines.runtime range', async () => {
  const project = prepare({
    devEngines: {
      runtime: {
        name: 'node',
        version: '^24.0.0',
        onFail: 'download',
      },
    },
  })

  await execPnpm(['install', '--lockfile-only'])
  await execPnpm(['add', '--save-optional', '--config.runtime=false', '@pnpm.e2e/requires-node-24-1@1.0.0'])

  expect(project.readModulesManifest()?.skipped).toStrictEqual([])
  project.has('@pnpm.e2e/requires-node-24-1')
})

test('an explicit nodeVersion takes priority over the Node.js version locked for devEngines.runtime', async () => {
  const project = prepare({
    dependencies: {
      dependency: 'file:dependency',
    },
    devEngines: {
      runtime: {
        name: 'node',
        version: '^24.0.0',
        onFail: 'download',
      },
    },
  })
  writeYamlFileSync('pnpm-workspace.yaml', {
    engineStrict: true,
    nodeVersion: '20.0.0',
  })
  fs.mkdirSync('dependency')
  fs.writeFileSync('dependency/package.json', JSON.stringify({
    name: 'dependency',
    version: '1.0.0',
    engines: {
      node: '<21',
    },
  }))

  await execPnpm(['install', '--lockfile-only'])
  await execPnpm(['install', '--frozen-lockfile', '--no-runtime'])

  project.has('dependency')
})

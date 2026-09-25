import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepare, preparePackages } from '@pnpm/prepare'
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

const recordNodeVersion = 'node -e "require(\'fs\').writeFileSync(\'node-version\', process.version)"'

skipOnWindows('dependency builds in the global virtual store run the root project runtime', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        name: 'root',
        version: '1.0.0',
        private: true,
        dependencies: { dep: 'file:dep' },
        devEngines: {
          runtime: {
            name: 'node',
            version: '22.19.0',
            onFail: 'download',
          },
        },
      },
    },
    {
      location: 'dep',
      package: { name: 'dep', version: '1.0.0', scripts: { postinstall: recordNodeVersion } },
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', {
    allowBuilds: { 'dep@file:dep': true },
    enableGlobalVirtualStore: true,
    scriptShell: '/bin/sh',
  })

  await execPnpm(['install', '--lockfile-only'])
  await execPnpm(['install', '--frozen-lockfile', '--prefer-offline'], { env: { PATH: '' } })
  expect(fs.readFileSync(path.join(fs.realpathSync('node_modules/dep'), 'node-version'), 'utf8')).toBe('v22.19.0')
})

// A filtered install that leaves the root project out still builds
// dependencies with the root project's runtime, the one that keys their
// side-effects cache entries.
skipOnWindows('a filtered install builds dependencies with the root project runtime', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        name: 'root',
        version: '1.0.0',
        private: true,
        devEngines: {
          runtime: {
            name: 'node',
            version: '22.19.0',
            onFail: 'download',
          },
        },
      },
    },
    {
      location: 'app',
      package: { name: 'app', version: '1.0.0', dependencies: { dep: 'file:../dep' } },
    },
    {
      location: 'dep',
      package: { name: 'dep', version: '1.0.0', scripts: { postinstall: recordNodeVersion } },
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', {
    allowBuilds: { 'dep@file:dep': true },
    packages: ['app'],
    scriptShell: '/bin/sh',
  })

  await execPnpm(['install', '--lockfile-only'])
  await execPnpm(['install', '--frozen-lockfile', '--prefer-offline', '--filter', 'app'], { env: { PATH: '' } })
  expect(fs.readFileSync('app/node_modules/dep/node-version', 'utf8')).toBe('v22.19.0')
})

// The slot hash does not record the workspace root's bins, so a build that
// another project reuses must not depend on them.
test('dependency build scripts in the global virtual store do not see the workspace root bins', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        name: 'root',
        version: '1.0.0',
        private: true,
        dependencies: { tool: 'file:tool', dep: 'file:dep' },
      },
    },
    {
      location: 'tool',
      package: {
        name: 'tool',
        version: '1.0.0',
        bin: { 'gvs-root-tool': 'cli.js' },
      },
    },
    {
      location: 'dep',
      package: {
        name: 'dep',
        version: '1.0.0',
        scripts: {
          postinstall: 'gvs-root-tool || node -e "require(\'fs\').writeFileSync(\'root-tool-missing\', \'\')"',
        },
      },
    },
  ])
  fs.writeFileSync('tool/cli.js', "#!/usr/bin/env node\nrequire('fs').writeFileSync('root-tool-ran', '')\n")
  writeYamlFileSync('pnpm-workspace.yaml', {
    allowBuilds: { 'dep@file:dep': true },
    enableGlobalVirtualStore: true,
  })

  await execPnpm(['install'])
  // The first install builds before it links the root's bins, so rebuild
  // once they are in place.
  await execPnpm(['rebuild', 'dep'])

  const depDir = fs.realpathSync('node_modules/dep')
  expect(fs.existsSync(path.join(depDir, 'root-tool-missing'))).toBe(true)
  expect(fs.existsSync(path.join(depDir, 'root-tool-ran'))).toBe(false)
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

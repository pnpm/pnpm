import PnpmError from '@pnpm/error'
import { recursive } from '@pnpm/plugin-commands-recursive'
import { preparePackages } from '@pnpm/prepare'
import makeDir = require('make-dir')
import fs = require('mz/fs')
import path = require('path')
import test = require('tape')
import writeJsonFile = require('write-json-file')
import writeYamlFile = require('write-yaml-file')
import { DEFAULT_OPTS } from './utils'

test('recursive install/uninstall', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  await recursive.handler(['install'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  t.ok(projects['project-1'].requireModule('is-positive'))
  t.ok(projects['project-2'].requireModule('is-negative'))
  await projects['project-2'].has('is-negative')

  await recursive.handler(['add', 'noop'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  t.ok(projects['project-1'].requireModule('noop'))
  t.ok(projects['project-2'].requireModule('noop'))

  await recursive.handler(['remove', 'is-negative'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  await projects['project-2'].hasNot('is-negative')

  t.end()
})

// Created to cover the issue described in https://github.com/pnpm/pnpm/issues/1253
test('recursive install with package that has link', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
        'project-2': 'link:../project-2',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  await recursive.handler(['install'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  t.ok(projects['project-1'].requireModule('is-positive'))
  t.ok(projects['project-1'].requireModule('project-2/package.json'))
  t.ok(projects['project-2'].requireModule('is-negative'))
  t.end()
})

test('running `pnpm recursive` on a subset of packages', async t => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['project-1'] })

  await recursive.handler(['install'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  await projects['project-1'].has('is-positive')
  await projects['project-2'].hasNot('is-negative')
  t.end()
})

test('running `pnpm recursive` only for packages in subdirectories of cwd', async t => {
  const projects = preparePackages(t, [
    {
      location: 'packages/project-1',
      package: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
        },
      },
    },
    {
      location: 'packages/project-2',
      package: {
        name: 'project-2',
        version: '1.0.0',

        dependencies: {
          'is-negative': '1.0.0',
        },
      }
    },
    {
      location: 'root-project',
      package: {
        name: 'root-project',
        version: '1.0.0',

        dependencies: {
          'debug': '*',
        },
      }
    }
  ])

  await makeDir('node_modules')
  process.chdir('packages')

  await recursive.handler(['install'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  await projects['project-1'].has('is-positive')
  await projects['project-2'].has('is-negative')
  await projects['root-project'].hasNot('debug')
  t.end()
})

test('recursive installation fails when installation in one of the packages fails', async t => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'this-pkg-does-not-exist': '100.100.100',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  let err!: PnpmError
  try {
    await recursive.handler(['install'], {
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    })
  } catch (_err) {
    err = _err
  }
  t.equal(err.code, 'ERR_PNPM_RECURSIVE_FAIL')
  t.end()
})

test('second run of `recursive install` after package.json has been edited manually', async t => {
  const projects = preparePackages(t, [
    {
      name: 'is-negative',
      version: '1.0.0',

      dependencies: {
        'is-positive': '2.0.0',
      },
    },
    {
      name: 'is-positive',
      version: '1.0.0',
    },
  ])

  await recursive.handler(['install'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  await writeJsonFile('is-negative/package.json', {
    name: 'is-negative',
    version: '1.0.0',

    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  await recursive.handler(['install'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  t.ok(projects['is-negative'].requireModule('is-positive/package.json'))
  t.end()
})

test('recursive --filter ignore excluded packages', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
        'project-2': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        minimatch: '*',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', {
    packages: [
      '**',
      '!project-1'
    ],
  })

  await recursive.handler(['install'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    filter: ['project-1...'],
  })

  projects['project-1'].hasNot('is-positive')
  projects['project-2'].hasNot('is-negative')
  projects['project-3'].hasNot('minimatch')
  t.end()
})

test('recursive filter multiple times', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
        'project-2': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        minimatch: '*',
      },
    },
  ])

  await recursive.handler(['install'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    filter: ['project-1', 'project-2'],
  })

  projects['project-1'].has('is-positive')
  projects['project-2'].has('is-negative')
  projects['project-3'].hasNot('minimatch')
  t.end()
})

test('recursive install --no-bail', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        '@pnpm/this-does-not-exist': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  let err!: PnpmError
  try {
    await recursive.handler(['install'], {
      ...DEFAULT_OPTS,
      bail: false,
      dir: process.cwd(),
    })
  } catch (_err) {
    err = _err
  }

  t.equal(err.code, 'ERR_PNPM_RECURSIVE_FAIL')

  t.ok(projects['project-2'].requireModule('is-negative'))
  t.end()
})

test('installing with "workspace=true" should work even if link-workspace-packages is off and save-workspace-protocol is false', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'project-2': '0.0.0',
      },
    },
    {
      name: 'project-2',
      version: '2.0.0',
    },
  ])

  await recursive.handler(['update', 'project-2'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    linkWorkspacePackages: false,
    lockfileDir: process.cwd(),
    saveWorkspaceProtocol: false,
    sharedWorkspaceLockfile: true,
    workspace: true,
    workspaceDir: process.cwd(),
  })

  {
    const pkg = await import(path.resolve('project-1/package.json'))
    t.deepEqual(pkg && pkg.dependencies, { 'project-2': 'workspace:^2.0.0' })
  }
  {
    const pkg = await import(path.resolve('project-2/package.json'))
    t.notOk(pkg.dependencies)
  }

  await projects['project-1'].has('project-2')

  t.end()
})

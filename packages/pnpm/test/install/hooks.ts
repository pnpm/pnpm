import { promises as fs } from 'fs'
import path from 'path'
import { Lockfile } from '@pnpm/lockfile-types'
import prepare, { preparePackages } from '@pnpm/prepare'
import readYamlFile from 'read-yaml-file'
import writeYamlFile from 'write-yaml-file'
import {
  addDistTag,
  execPnpm,
  execPnpmSync,
} from '../utils'

test('readPackage hook', async () => {
  const project = prepare()

  await fs.writeFile('.pnpmfile.cjs', `
    'use strict'
    module.exports = {
      hooks: {
        readPackage (pkg) {
          if (pkg.name === 'pkg-with-1-dep') {
            pkg.dependencies['dep-of-pkg-with-1-dep'] = '100.0.0'
          }
          return pkg
        }
      }
    }
  `, 'utf8')

  // w/o the hook, 100.1.0 would be installed
  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await execPnpm(['install', 'pkg-with-1-dep'])

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})

test('readPackage async hook', async () => {
  const project = prepare()

  await fs.writeFile('.pnpmfile.cjs', `
    'use strict'
    module.exports = {
      hooks: {
        async readPackage (pkg) {
          if (pkg.name === 'pkg-with-1-dep') {
            pkg.dependencies['dep-of-pkg-with-1-dep'] = '100.0.0'
          }
          return pkg
        }
      }
    }
  `, 'utf8')

  // w/o the hook, 100.1.0 would be installed
  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await execPnpm(['install', 'pkg-with-1-dep'])

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})

test('readPackage hook makes installation fail if it does not return the modified package manifests', async () => {
  prepare()

  await fs.writeFile('.pnpmfile.cjs', `
    'use strict'
    module.exports = {
      hooks: {
        readPackage (pkg) {}
      }
    }
  `, 'utf8')

  const result = execPnpmSync(['install', 'pkg-with-1-dep'])

  expect(result.status).toBe(1)
})

test('readPackage hook from custom location', async () => {
  const project = prepare()

  await fs.writeFile('pnpm.js', `
    'use strict'
    module.exports = {
      hooks: {
        readPackage (pkg) {
          if (pkg.name === 'pkg-with-1-dep') {
            pkg.dependencies['dep-of-pkg-with-1-dep'] = '100.0.0'
          }
          return pkg
        }
      }
    }
  `, 'utf8')

  // w/o the hook, 100.1.0 would be installed
  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await execPnpm(['install', 'pkg-with-1-dep', '--pnpmfile', 'pnpm.js'])

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})

test('readPackage hook from global pnpmfile', async () => {
  const project = prepare()

  await fs.writeFile('../.pnpmfile.cjs', `
    'use strict'
    module.exports = {
      hooks: {
        readPackage (pkg) {
          if (pkg.name === 'pkg-with-1-dep') {
            pkg.dependencies['dep-of-pkg-with-1-dep'] = '100.0.0'
          }
          return pkg
        }
      }
    }
  `, 'utf8')

  // w/o the hook, 100.1.0 would be installed
  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await execPnpm(['install', 'pkg-with-1-dep', '--global-pnpmfile', path.resolve('..', '.pnpmfile.cjs')])

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})

test('readPackage hook from global pnpmfile and local pnpmfile', async () => {
  const project = prepare()

  await fs.writeFile('../.pnpmfile.cjs', `
    'use strict'
    module.exports = {
      hooks: {
        readPackage (pkg) {
          if (pkg.name === 'pkg-with-1-dep') {
            pkg.dependencies['dep-of-pkg-with-1-dep'] = '100.0.0'
            pkg.dependencies['is-positive'] = '3.0.0'
          }
          return pkg
        }
      }
    }
  `, 'utf8')

  await fs.writeFile('.pnpmfile.cjs', `
    'use strict'
    module.exports = {
      hooks: {
        readPackage (pkg) {
          if (pkg.name === 'pkg-with-1-dep') {
            pkg.dependencies['is-positive'] = '1.0.0'
          }
          return pkg
        }
      }
    }
  `, 'utf8')

  // w/o the hook, 100.1.0 would be installed
  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await execPnpm(['install', 'pkg-with-1-dep', '--global-pnpmfile', path.resolve('..', '.pnpmfile.cjs')])

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
  await project.storeHas('is-positive', '1.0.0')
})

test('readPackage async hook from global pnpmfile and local pnpmfile', async () => {
  const project = prepare()

  await fs.writeFile('../.pnpmfile.cjs', `
    'use strict'
    module.exports = {
      hooks: {
        async readPackage (pkg) {
          if (pkg.name === 'pkg-with-1-dep') {
            pkg.dependencies['dep-of-pkg-with-1-dep'] = '100.0.0'
            pkg.dependencies['is-positive'] = '3.0.0'
          }
          return pkg
        }
      }
    }
  `, 'utf8')

  await fs.writeFile('.pnpmfile.cjs', `
    'use strict'
    module.exports = {
      hooks: {
        async readPackage (pkg) {
          if (pkg.name === 'pkg-with-1-dep') {
            pkg.dependencies['is-positive'] = '1.0.0'
          }
          return pkg
        }
      }
    }
  `, 'utf8')

  // w/o the hook, 100.1.0 would be installed
  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await execPnpm(['install', 'pkg-with-1-dep', '--global-pnpmfile', path.resolve('..', '.pnpmfile.cjs')])

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
  await project.storeHas('is-positive', '1.0.0')
})

test('readPackage hook from pnpmfile at root of workspace', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
  ])

  const pnpmfile = `
    module.exports = { hooks: { readPackage } }
    function readPackage (pkg) {
      pkg.dependencies = pkg.dependencies || {}
      pkg.dependencies['dep-of-pkg-with-1-dep'] = '100.1.0'
      return pkg
    }
  `
  await fs.writeFile('.pnpmfile.cjs', pnpmfile, 'utf8')

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['project-1'] })

  const storeDir = path.resolve('store')

  await execPnpm(['recursive', 'install', '--store-dir', storeDir])

  process.chdir('project-1')

  await execPnpm(['install', 'is-negative@1.0.0', '--store-dir', storeDir])

  await projects['project-1'].has('is-negative')
  await projects['project-1'].has('is-positive')

  process.chdir('..')

  const lockfile = await readYamlFile<Lockfile>('pnpm-lock.yaml')
  expect(lockfile.packages!['/is-positive/1.0.0'].dependencies).toStrictEqual({
    'dep-of-pkg-with-1-dep': '100.1.0',
  })
  expect(lockfile.packages!['/is-negative/1.0.0'].dependencies).toStrictEqual({
    'dep-of-pkg-with-1-dep': '100.1.0',
  })
  /* eslint-enable @typescript-eslint/no-unnecessary-type-assertion */
})

test('readPackage hook during update', async () => {
  const project = prepare({
    dependencies: {
      'pkg-with-1-dep': '*',
    },
  })

  await fs.writeFile('.pnpmfile.cjs', `
    'use strict'
    module.exports = {
      hooks: {
        readPackage (pkg) {
          if (pkg.name === 'pkg-with-1-dep') {
            pkg.dependencies['dep-of-pkg-with-1-dep'] = '100.0.0'
          }
          return pkg
        }
      }
    }
  `, 'utf8')

  // w/o the hook, 100.1.0 would be installed
  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await execPnpm(['update'])

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})

test('prints meaningful error when there is syntax error in .pnpmfile.cjs', async () => {
  prepare()

  await fs.writeFile('.pnpmfile.cjs', '/boom', 'utf8')

  const proc = execPnpmSync(['install', 'pkg-with-1-dep'])

  expect(proc.stderr.toString()).toContain('SyntaxError: Invalid regular expression: missing /')
  expect(proc.status).toBe(1)
})

test('fails when .pnpmfile.cjs requires a non-existed module', async () => {
  prepare()

  await fs.writeFile('.pnpmfile.cjs', 'module.exports = require("./this-does-node-exist")', 'utf8')

  const proc = execPnpmSync(['install', 'pkg-with-1-dep'])

  expect(proc.stdout.toString()).toContain('Error during pnpmfile execution')
  expect(proc.status).toBe(1)
})

test('ignore .pnpmfile.cjs when --ignore-pnpmfile is used', async () => {
  const project = prepare()

  await fs.writeFile('.pnpmfile.cjs', `
    'use strict'
    module.exports = {
      hooks: {
        readPackage (pkg) {
          if (pkg.name === 'pkg-with-1-dep') {
            pkg.dependencies['dep-of-pkg-with-1-dep'] = '100.0.0'
          }
          return pkg
        }
      }
    }
  `, 'utf8')

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await execPnpm(['install', 'pkg-with-1-dep', '--ignore-pnpmfile'])

  await project.storeHas('dep-of-pkg-with-1-dep', '100.1.0')
})

test('ignore .pnpmfile.cjs during update when --ignore-pnpmfile is used', async () => {
  const project = prepare({
    dependencies: {
      'pkg-with-1-dep': '*',
    },
  })

  await fs.writeFile('.pnpmfile.cjs', `
    'use strict'
    module.exports = {
      hooks: {
        readPackage (pkg) {
          if (pkg.name === 'pkg-with-1-dep') {
            pkg.dependencies['dep-of-pkg-with-1-dep'] = '100.0.0'
          }
          return pkg
        }
      }
    }
  `, 'utf8')

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await execPnpm(['update', '--ignore-pnpmfile'])

  await project.storeHas('dep-of-pkg-with-1-dep', '100.1.0')
})

test('pnpmfile: pass log function to readPackage hook', async () => {
  const project = prepare()

  await fs.writeFile('.pnpmfile.cjs', `
    'use strict'
    module.exports = {
      hooks: {
        readPackage (pkg, context) {
          if (pkg.name === 'pkg-with-1-dep') {
            pkg.dependencies['dep-of-pkg-with-1-dep'] = '100.0.0'
            context.log('dep-of-pkg-with-1-dep pinned to 100.0.0')
          }
          return pkg
        }
      }
    }
  `, 'utf8')

  // w/o the hook, 100.1.0 would be installed
  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  const proc = execPnpmSync(['install', 'pkg-with-1-dep', '--reporter', 'ndjson'])

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  const outputs = proc.stdout.toString().split(/\r?\n/)

  const hookLog = outputs.filter(Boolean)
    .map((output) => JSON.parse(output))
    .find((log) => log.name === 'pnpm:hook')

  expect(hookLog).toBeTruthy()
  expect(hookLog.prefix).toBeTruthy()
  expect(hookLog.from).toBeTruthy()
  expect(hookLog.hook).toBe('readPackage')
  expect(hookLog.message).toBe('dep-of-pkg-with-1-dep pinned to 100.0.0')
})

test('pnpmfile: pass log function to readPackage hook of global and local pnpmfile', async () => {
  const project = prepare()

  await fs.writeFile('../.pnpmfile.cjs', `
    'use strict'
    module.exports = {
      hooks: {
        readPackage (pkg, context) {
          if (pkg.name === 'pkg-with-1-dep') {
            pkg.dependencies['dep-of-pkg-with-1-dep'] = '100.0.0'
            pkg.dependencies['is-positive'] = '3.0.0'
            context.log('is-positive pinned to 3.0.0')
          }
          return pkg
        }
      }
    }
  `, 'utf8')

  await fs.writeFile('.pnpmfile.cjs', `
    'use strict'
    module.exports = {
      hooks: {
        readPackage (pkg, context) {
          if (pkg.name === 'pkg-with-1-dep') {
            pkg.dependencies['is-positive'] = '1.0.0'
            context.log('is-positive pinned to 1.0.0')
          }
          return pkg
        }
      }
    }
  `, 'utf8')

  // w/o the hook, 100.1.0 would be installed
  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  const proc = execPnpmSync(['install', 'pkg-with-1-dep', '--global-pnpmfile', path.resolve('..', '.pnpmfile.cjs'), '--reporter', 'ndjson'])

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
  await project.storeHas('is-positive', '1.0.0')

  const outputs = proc.stdout.toString().split(/\r?\n/)

  const hookLogs = outputs.filter(Boolean)
    .map((output) => JSON.parse(output))
    .filter((log) => log.name === 'pnpm:hook')

  expect(hookLogs[0]).toBeTruthy()
  expect(hookLogs[0].prefix).toBeTruthy()
  expect(hookLogs[0].from).toBeTruthy()
  expect(hookLogs[0].hook).toBe('readPackage')
  expect(hookLogs[0].message).toBe('is-positive pinned to 3.0.0')

  expect(hookLogs[1]).toBeTruthy()
  expect(hookLogs[1].prefix).toBeTruthy()
  expect(hookLogs[1].from).toBeTruthy()
  expect(hookLogs[1].hook).toBe('readPackage')
  expect(hookLogs[1].message).toBe('is-positive pinned to 1.0.0')

  expect(hookLogs[0].prefix).toBe(hookLogs[1].prefix)
  expect(hookLogs[0].from).not.toBe(hookLogs[1].from)
})

test('pnpmfile: run afterAllResolved hook', async () => {
  prepare()

  await fs.writeFile('.pnpmfile.cjs', `
    'use strict'
    module.exports = {
      hooks: {
        afterAllResolved (lockfile, context) {
          context.log('All resolved')
          return lockfile
        }
      }
    }
  `, 'utf8')

  const proc = execPnpmSync(['install', 'pkg-with-1-dep', '--reporter', 'ndjson'])

  const outputs = proc.stdout.toString().split(/\r?\n/)

  const hookLog = outputs.filter(Boolean)
    .map((output) => JSON.parse(output))
    .find((log) => log.name === 'pnpm:hook')

  expect(hookLog).toBeTruthy()
  expect(hookLog.prefix).toBeTruthy()
  expect(hookLog.from).toBeTruthy()
  expect(hookLog.hook).toBe('afterAllResolved')
  expect(hookLog.message).toBe('All resolved')
})

test('pnpmfile: run async afterAllResolved hook', async () => {
  prepare()

  await fs.writeFile('.pnpmfile.cjs', `
    'use strict'
    module.exports = {
      hooks: {
        async afterAllResolved (lockfile, context) {
          context.log('All resolved')
          return lockfile
        }
      }
    }
  `, 'utf8')

  const proc = execPnpmSync(['install', 'pkg-with-1-dep', '--reporter', 'ndjson'])

  const outputs = proc.stdout.toString().split(/\r?\n/)

  const hookLog = outputs.filter(Boolean)
    .map((output) => JSON.parse(output))
    .find((log) => log.name === 'pnpm:hook')

  expect(hookLog).toBeTruthy()
  expect(hookLog.prefix).toBeTruthy()
  expect(hookLog.from).toBeTruthy()
  expect(hookLog.hook).toBe('afterAllResolved')
  expect(hookLog.message).toBe('All resolved')
})

test('readPackage hook normalizes the package manifest', async () => {
  prepare()

  await fs.writeFile('.pnpmfile.cjs', `
    'use strict'
    module.exports = {
      hooks: {
        readPackage (pkg) {
          if (pkg.name === 'dep-of-pkg-with-1-dep') {
            pkg.dependencies['is-positive'] = '*'
            pkg.optionalDependencies['is-negative'] = '*'
            pkg.peerDependencies['is-negative'] = '*'
            pkg.devDependencies['is-positive'] = '*'
          }
          return pkg
        }
      }
    }
  `, 'utf8')

  await execPnpm(['install', 'dep-of-pkg-with-1-dep'])
})

test('readPackage hook overrides project package', async () => {
  const project = prepare({
    name: 'test-read-package-hook',
  })

  await fs.writeFile('.pnpmfile.cjs', `
    'use strict'
    module.exports = {
      hooks: {
        readPackage (pkg) {
          switch (pkg.name) {
            case 'test-read-package-hook':
              pkg.dependencies = { 'is-positive': '1.0.0' }
              break
          }
          return pkg
        }
      }
    }
  `, 'utf8')

  await execPnpm(['install'])

  await project.has('is-positive')

  const pkg = await import(path.resolve('package.json'))
  expect(pkg.dependencies).toBeFalsy()
})

test('readPackage hook is used during removal inside a workspace', async () => {
  preparePackages([
    {
      name: 'project',
      version: '1.0.0',

      dependencies: {
        abc: '1.0.0',
        'is-negative': '1.0.0',
        'is-positive': '1.0.0',
        'peer-a': '1.0.0',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['project-1'] })
  await fs.writeFile('.pnpmfile.cjs', `
    'use strict'
    module.exports = {
      hooks: {
        readPackage (pkg) {
          switch (pkg.name) {
            case 'abc':
              pkg.peerDependencies['is-negative'] = '1.0.0'
              break
          }
          return pkg
        }
      }
    }
  `, 'utf8')

  process.chdir('project')
  await execPnpm(['install', '--no-strict-peer-dependencies'])
  await execPnpm(['uninstall', 'is-positive', '--no-strict-peer-dependencies'])

  process.chdir('..')
  const lockfile = await readYamlFile<Lockfile>('pnpm-lock.yaml')
  expect(lockfile.packages!['/abc/1.0.0_vt2fli7reel7pfbmpdhs3d7fya'].peerDependencies!['is-negative']).toBe('1.0.0')
})

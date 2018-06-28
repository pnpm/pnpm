import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  prepare,
  addDistTag,
  execPnpm,
  execPnpmSync,
} from '../utils'
import path = require('path')
import fs = require('mz/fs')

const test = promisifyTape(tape)

test('readPackage hook', async (t: tape.Test) => {
  const project = prepare(t)

  await fs.writeFile('pnpmfile.js', `
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

  await execPnpm('install', 'pkg-with-1-dep')

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})

test('readPackage hook makes installation fail if it does not return the modified package manifests', async (t: tape.Test) => {
  const project = prepare(t)

  await fs.writeFile('pnpmfile.js', `
    'use strict'
    module.exports = {
      hooks: {
        readPackage (pkg) {}
      }
    }
  `, 'utf8')

  const result = await execPnpmSync('install', 'pkg-with-1-dep')

  t.equal(result.status, 1, 'installation failed')
})

test('readPackage hook from custom location', async (t: tape.Test) => {
  const project = prepare(t)

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

  await execPnpm('install', 'pkg-with-1-dep', '--pnpmfile', 'pnpm.js')

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})

test('readPackage hook from global pnpmfile', async (t: tape.Test) => {
  const project = prepare(t)

  await fs.writeFile('../pnpmfile.js', `
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

  await execPnpm('install', 'pkg-with-1-dep', '--global-pnpmfile', path.resolve('..', 'pnpmfile.js'))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})

test('readPackage hook from global pnpmfile and local pnpmfile', async (t: tape.Test) => {
  const project = prepare(t)

  await fs.writeFile('../pnpmfile.js', `
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

  await fs.writeFile('pnpmfile.js', `
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

  await execPnpm('install', 'pkg-with-1-dep', '--global-pnpmfile', path.resolve('..', 'pnpmfile.js'))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
  await project.storeHas('is-positive', '1.0.0')
})

test('readPackage hook during update', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'pkg-with-1-dep': '*',
    },
  })

  await fs.writeFile('pnpmfile.js', `
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

  await execPnpm('update')

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})

test('prints meaningful error when there is syntax error in pnpmfile.js', async (t: tape.Test) => {
  const project = prepare(t)

  await fs.writeFile('pnpmfile.js', '/boom', 'utf8')

  const proc = execPnpmSync('install', 'pkg-with-1-dep')

  t.ok(proc.stderr.toString().indexOf('SyntaxError: Invalid regular expression: missing /') !== -1)
  t.equal(proc.status, 1)
})

test('fails when pnpmfile.js requires a non-existend module', async (t: tape.Test) => {
  const project = prepare(t)

  await fs.writeFile('pnpmfile.js', 'module.exports = require("./this-does-node-exist")', 'utf8')

  const proc = execPnpmSync('install', 'pkg-with-1-dep')

  t.ok(proc.stdout.toString().indexOf('Error during pnpmfile execution') !== -1)
  t.equal(proc.status, 1)
})

test('ignore pnpmfile.js when --ignore-pnpmfile is used', async (t: tape.Test) => {
  const project = prepare(t)

  await fs.writeFile('pnpmfile.js', `
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

  await execPnpm('install', 'pkg-with-1-dep', '--ignore-pnpmfile')

  await project.storeHas('dep-of-pkg-with-1-dep', '100.1.0')
})

test('ignore pnpmfile.js during update when --ignore-pnpmfile is used', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'pkg-with-1-dep': '*',
    },
  })

  await fs.writeFile('pnpmfile.js', `
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

  await execPnpm('update', '--ignore-pnpmfile')

  await project.storeHas('dep-of-pkg-with-1-dep', '100.1.0')
})

test('pnpmfile: pass log function to readPackage hook', async (t: tape.Test) => {
  const project = prepare(t)

  await fs.writeFile('pnpmfile.js', `
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

  const proc = execPnpmSync('install', 'pkg-with-1-dep', '--reporter', 'ndjson')

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  const outputs = proc.stdout.toString().split(/\r?\n/)

  const hookLog = outputs.filter(Boolean)
    .map((output) => JSON.parse(output))
    .find((log) => log.name === 'pnpm:hook')

  t.ok(hookLog, 'logged')
  t.ok(hookLog.prefix, 'logged prefix')
  t.ok(hookLog.from, 'logged the hook source')
  t.equal(hookLog.hook, 'readPackage', 'logged hook name')
  t.equal(hookLog.message, 'dep-of-pkg-with-1-dep pinned to 100.0.0', 'logged the message')
})

test('pnpmfile: pass log function to readPackage hook of global and local pnpmfile', async (t: tape.Test) => {
  const project = prepare(t)

  await fs.writeFile('../pnpmfile.js', `
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

  await fs.writeFile('pnpmfile.js', `
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

  const proc = execPnpmSync('install', 'pkg-with-1-dep', '--global-pnpmfile', path.resolve('..', 'pnpmfile.js'), '--reporter', 'ndjson')

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
  await project.storeHas('is-positive', '1.0.0')

  const outputs = proc.stdout.toString().split(/\r?\n/)

  const hookLogs = outputs.filter(Boolean)
    .map((output) => JSON.parse(output))
    .filter((log) => log.name === 'pnpm:hook')

  t.ok(hookLogs[0], 'logged')
  t.ok(hookLogs[0].prefix, 'logged prefix')
  t.ok(hookLogs[0].from, 'logged the hook source')
  t.equal(hookLogs[0].hook, 'readPackage', 'logged hook name')
  t.equal(hookLogs[0].message, 'is-positive pinned to 3.0.0', 'logged the message')

  t.ok(hookLogs[1], 'logged')
  t.ok(hookLogs[1].prefix, 'logged prefix')
  t.ok(hookLogs[1].from, 'logged the hook source')
  t.equal(hookLogs[1].hook, 'readPackage', 'logged hook name')
  t.equal(hookLogs[1].message, 'is-positive pinned to 1.0.0', 'logged the message')

  t.ok(hookLogs[0].prefix === hookLogs[1].prefix, 'logged prefix correctly')
  t.ok(hookLogs[0].from !== hookLogs[1].from, 'logged from correctly')
})

test('pnpmfile: run afterAllResolved hook', async (t: tape.Test) => {
  const project = prepare(t)

  await fs.writeFile('pnpmfile.js', `
    'use strict'
    module.exports = {
      hooks: {
        afterAllResolved (shr, context) {
          context.log('All resolved')
        }
      }
    }
  `, 'utf8')

  const proc = execPnpmSync('install', 'pkg-with-1-dep', '--reporter', 'ndjson')

  const outputs = proc.stdout.toString().split(/\r?\n/)

  const hookLog = outputs.filter(Boolean)
    .map((output) => JSON.parse(output))
    .find((log) => log.name === 'pnpm:hook')

  t.ok(hookLog, 'logged')
  t.ok(hookLog.prefix, 'logged prefix')
  t.ok(hookLog.from, 'logged the hook source')
  t.equal(hookLog.hook, 'afterAllResolved', 'logged hook name')
  t.equal(hookLog.message, 'All resolved', 'logged the message')
})

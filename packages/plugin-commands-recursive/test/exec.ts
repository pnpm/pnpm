import PnpmError from '@pnpm/error'
import { recursive } from '@pnpm/plugin-commands-recursive'
import { preparePackages } from '@pnpm/prepare'
import rimraf = require('@zkochan/rimraf')
import fs = require('mz/fs')
import path = require('path')
import test = require('tape')
import { DEFAULT_OPTS } from './utils'

test('pnpm recursive exec', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: `node -e "process.stdout.write('project-1')" | json-append ../output1.json && node -e "process.stdout.write('project-1')" | json-append ../output2.json`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-1': '1'
      },
      scripts: {
        build: `node -e "process.stdout.write('project-2')" | json-append ../output1.json`,
        postbuild: `node -e "process.stdout.write('project-2-postbuild')" | json-append ../output1.json`,
        prebuild: `node -e "process.stdout.write('project-2-prebuild')" | json-append ../output1.json`,
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-1': '1'
      },
      scripts: {
        build: `node -e "process.stdout.write('project-3')" | json-append ../output2.json`,
      },
    },
  ])

  await recursive.handler(['install'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })
  await recursive.handler(['exec', 'npm', 'run', 'build'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  const outputs1 = await import(path.resolve('output1.json')) as string[]
  const outputs2 = await import(path.resolve('output2.json')) as string[]

  t.deepEqual(outputs1, ['project-1', 'project-2-prebuild', 'project-2', 'project-2-postbuild'])
  t.deepEqual(outputs2, ['project-1', 'project-3'])

  t.end()
})

test('pnpm recursive exec sets PNPM_PACKAGE_NAME env var', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'foo',
      version: '1.0.0',
    },
  ])

  await recursive.handler(['exec', 'node', '-e', `require('fs').writeFileSync('pkgname', process.env.PNPM_PACKAGE_NAME, 'utf8')`], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  t.equal(await fs.readFile('foo/pkgname', 'utf8'), 'foo', '$PNPM_PACKAGE_NAME is correct')
  t.end()
})

test('testing the bail config with "pnpm recursive exec"', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: `node -e "process.stdout.write('project-1')" | json-append ../output.json`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-1': '1'
      },
      scripts: {
        build: `exit 1 && node -e "process.stdout.write('project-2')" | json-append ../output.json`,
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-1': '1'
      },
      scripts: {
        build: `node -e "process.stdout.write('project-3')" | json-append ../output.json`,
      },
    },
  ])

  await recursive.handler(['install'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  let failed = false
  let err1!: PnpmError
  try {
    await recursive.handler(['exec', 'npm', 'run', 'build', '--no-bail'], {
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    })
  } catch (_err) {
    err1 = _err
    failed = true
  }
  t.equal(err1.code, 'ERR_PNPM_RECURSIVE_FAIL')
  t.ok(failed, 'recursive exec failed with --no-bail')

  const outputs = await import(path.resolve('output.json')) as string[]
  t.deepEqual(outputs, ['project-1', 'project-3'], 'error skipped')

  await rimraf('./output.json')

  failed = false
  let err2!: PnpmError
  try {
    await recursive.handler(['exec', 'npm', 'run', 'build'], {
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    })
  } catch (_err) {
    err2 = _err
    failed = true
  }

  t.equal(err2.code, 'ERR_PNPM_RECURSIVE_FAIL')
  t.ok(failed, 'recursive exec failed with --bail')
  t.end()
})

test('pnpm recursive exec --no-sort', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'a-dependent',
      version: '1.0.0',

      dependencies: {
        'b-dependency': '1.0.0',
        'json-append': '1',
      },
      scripts: {
        build: `node -e "process.stdout.write('a-dependent')" | json-append ../output.json`,
      },
    },
    {
      name: 'b-dependency',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        build: `node -e "process.stdout.write('b-dependency')" | json-append ../output.json`,
      },
    },
  ])

  await recursive.handler(['install'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    linkWorkspacePackages: true,
  })
  await recursive.handler(['exec', 'npm', 'run', 'build'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    sort: false,
    workspaceConcurrency: 1,
  })

  const outputs = await import(path.resolve('output.json')) as string[]

  t.deepEqual(outputs, ['a-dependent', 'b-dependency'])
  t.end()
})

/// <reference path="../../../__typings__/index.d.ts" />
import { promises as fs } from 'fs'
import path from 'path'
import { type PnpmError } from '@pnpm/error'
import { readProjects } from '@pnpm/filter-workspace-packages'
import {
  restart,
  run,
  test as testCommand,
} from '@pnpm/plugin-commands-script-runners'
import { prepare, preparePackages } from '@pnpm/prepare'
import execa from 'execa'
import isWindows from 'is-windows'
import writeYamlFile from 'write-yaml-file'
import { DEFAULT_OPTS, REGISTRY_URL } from './utils'

const pnpmBin = path.join(__dirname, '../../../pnpm/bin/pnpm.cjs')

test('pnpm run: returns correct exit code', async () => {
  prepare({
    scripts: {
      exit0: 'exit 0',
      exit1: 'exit 1',
    },
  })

  await run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    extraEnv: {},
    rawConfig: {},
  }, ['exit0'])

  let err!: Error & { errno: number }
  try {
    await run.handler({
      dir: process.cwd(),
      extraBinPaths: [],
      extraEnv: {},
      rawConfig: {},
    }, ['exit1'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.errno).toBe(1)
})

test('pnpm run --no-bail never fails', async () => {
  prepare({
    scripts: {
      exit1: 'node recordArgs && exit 1',
    },
  })
  await fs.writeFile('args.json', '[]', 'utf8')
  await fs.writeFile('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

  await run.handler({
    bail: false,
    dir: process.cwd(),
    extraBinPaths: [],
    extraEnv: {},
    rawConfig: {},
  }, ['exit1'])

  const { default: args } = await import(path.resolve('args.json'))
  expect(args).toStrictEqual([[]])
})

const RECORD_ARGS_FILE = 'require(\'fs\').writeFileSync(\'args.json\', JSON.stringify(require(\'./args.json\').concat([process.argv.slice(2)])), \'utf8\')'

test('run: pass the args to the command that is specified in the build script', async () => {
  prepare({
    scripts: {
      foo: 'node recordArgs',
      postfoo: 'node recordArgs',
      prefoo: 'node recordArgs',
    },
  })
  await fs.writeFile('args.json', '[]', 'utf8')
  await fs.writeFile('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

  await run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    extraEnv: {},
    rawConfig: {},
  }, ['foo', 'arg', '--flag=true', '--help', '-h'])

  const { default: args } = await import(path.resolve('args.json'))
  expect(args).toStrictEqual([['arg', '--flag=true', '--help', '-h']])
})

test('run: pass the args to the command that is specified in the build script of a package.yaml manifest', async () => {
  prepare({
    scripts: {
      foo: 'node recordArgs',
      postfoo: 'node recordArgs',
      prefoo: 'node recordArgs',
    },
  }, { manifestFormat: 'YAML' })
  await fs.writeFile('args.json', '[]', 'utf8')
  await fs.writeFile('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

  await run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    extraEnv: {},
    rawConfig: {},
  }, ['foo', 'arg', '--flag=true', '--help', '-h'])

  const { default: args } = await import(path.resolve('args.json'))
  expect(args).toStrictEqual([['arg', '--flag=true', '--help', '-h']])
})

test('test: pass the args to the command that is specified in the build script of a package.yaml manifest', async () => {
  prepare({
    scripts: {
      posttest: 'node recordArgs',
      pretest: 'node recordArgs',
      test: 'node recordArgs',
    },
  }, { manifestFormat: 'YAML' })
  await fs.writeFile('args.json', '[]', 'utf8')
  await fs.writeFile('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

  await testCommand.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    extraEnv: {},
    rawConfig: {},
  }, ['arg', '--flag=true', '--help', '-h'])

  const { default: args } = await import(path.resolve('args.json'))
  expect(args).toStrictEqual([['arg', '--flag=true', '--help', '-h']])
})

test('run start: pass the args to the command that is specified in the build script of a package.yaml manifest', async () => {
  prepare({
    scripts: {
      poststart: 'node recordArgs',
      prestart: 'node recordArgs',
      start: 'node recordArgs',
    },
  }, { manifestFormat: 'YAML' })
  await fs.writeFile('args.json', '[]', 'utf8')
  await fs.writeFile('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

  await run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    extraEnv: {},
    rawConfig: {},
  }, ['start', 'arg', '--flag=true', '--help', '-h'])

  const { default: args } = await import(path.resolve('args.json'))
  expect(args).toStrictEqual([['arg', '--flag=true', '--help', '-h']])
})

test('run stop: pass the args to the command that is specified in the build script of a package.yaml manifest', async () => {
  prepare({
    scripts: {
      poststop: 'node recordArgs',
      prestop: 'node recordArgs',
      stop: 'node recordArgs',
    },
  }, { manifestFormat: 'YAML' })
  await fs.writeFile('args.json', '[]', 'utf8')
  await fs.writeFile('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

  await run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    extraEnv: {},
    rawConfig: {},
  }, ['stop', 'arg', '--flag=true', '--help', '-h'])

  const { default: args } = await import(path.resolve('args.json'))
  expect(args).toStrictEqual([['arg', '--flag=true', '--help', '-h']])
})

test('restart: run stop, restart and start', async () => {
  prepare({
    scripts: {
      poststop: 'node -e "process.stdout.write(\'poststop\')" | json-append ./output.json',
      prestop: 'node -e "process.stdout.write(\'prestop\')" | json-append ./output.json',
      stop: 'node -e "process.stdout.write(\'stop\')" | json-append ./output.json',

      postrestart: 'node -e "process.stdout.write(\'postrestart\')" | json-append ./output.json',
      prerestart: 'node -e "process.stdout.write(\'prerestart\')" | json-append ./output.json',
      restart: 'node -e "process.stdout.write(\'restart\')" | json-append ./output.json',

      poststart: 'node -e "process.stdout.write(\'poststart\')" | json-append ./output.json',
      prestart: 'node -e "process.stdout.write(\'prestart\')" | json-append ./output.json',
      start: 'node -e "process.stdout.write(\'start\')" | json-append ./output.json',
    },
  })

  await execa('pnpm', ['add', 'json-append@1'])
  await restart.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    extraEnv: {},
    rawConfig: {},
  }, [])

  const { default: scriptsRan } = await import(path.resolve('output.json'))
  expect(scriptsRan).toStrictEqual([
    'stop',
    'restart',
    'start',
  ])
})

test('restart: run stop, restart and start and all the pre/post scripts', async () => {
  prepare({
    scripts: {
      poststop: 'node -e "process.stdout.write(\'poststop\')" | json-append ./output.json',
      prestop: 'node -e "process.stdout.write(\'prestop\')" | json-append ./output.json',
      stop: 'pnpm prestop && node -e "process.stdout.write(\'stop\')" | json-append ./output.json && pnpm poststop',

      postrestart: 'node -e "process.stdout.write(\'postrestart\')" | json-append ./output.json',
      prerestart: 'node -e "process.stdout.write(\'prerestart\')" | json-append ./output.json',
      restart: 'node -e "process.stdout.write(\'restart\')" | json-append ./output.json',

      poststart: 'node -e "process.stdout.write(\'poststart\')" | json-append ./output.json',
      prestart: 'node -e "process.stdout.write(\'prestart\')" | json-append ./output.json',
      start: 'node -e "process.stdout.write(\'start\')" | json-append ./output.json',
    },
  })

  await execa('pnpm', ['add', 'json-append@1'])
  await restart.handler({
    dir: process.cwd(),
    enablePrePostScripts: true,
    extraBinPaths: [],
    extraEnv: {},
    rawConfig: {},
  }, [])

  const { default: scriptsRan } = await import(path.resolve('output.json'))
  expect(scriptsRan).toStrictEqual([
    'prestop',
    'stop',
    'poststop',
    'prerestart',
    'restart',
    'postrestart',
    'prestart',
    'start',
    'poststart',
  ])
})

test('"pnpm run" prints the list of available commands', async () => {
  prepare({
    scripts: {
      foo: 'echo hi',
      test: 'ts-node test',
    },
  })

  const output = await run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    extraEnv: {},
    rawConfig: {},
  }, [])

  expect(output).toBe(`\
Lifecycle scripts:
  test
    ts-node test

Commands available via "pnpm run":
  foo
    echo hi`)
})

test('"pnpm run" prints the list of available commands, including commands of the root workspace project', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        dependencies: {
          'json-append': '1',
        },
        scripts: {
          build: 'echo root',
          test: 'test-all',
        },
      },
    },
    {
      name: 'foo',
      version: '1.0.0',

      scripts: {
        foo: 'echo hi',
        test: 'ts-node test',
      },
    },
  ])
  await writeYamlFile('pnpm-workspace.yaml', {})
  const workspaceDir = process.cwd()

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])

  {
    process.chdir('foo')
    const output = await run.handler({
      allProjects,
      dir: process.cwd(),
      extraBinPaths: [],
      extraEnv: {},
      rawConfig: {},
      selectedProjectsGraph,
      workspaceDir,
    }, [])

    expect(output).toBe(`\
Lifecycle scripts:
  test
    ts-node test

Commands available via "pnpm run":
  foo
    echo hi

Commands of the root workspace project (to run them, use "pnpm -w run"):
  build
    echo root
  test
    test-all`)
  }
  {
    process.chdir('..')
    const output = await run.handler({
      allProjects,
      dir: process.cwd(),
      extraBinPaths: [],
      extraEnv: {},
      rawConfig: {},
      selectedProjectsGraph,
      workspaceDir,
    }, [])

    expect(output).toBe(`\
Lifecycle scripts:
  test
    test-all

Commands available via "pnpm run":
  build
    echo root`)
  }
})

test('pnpm run does not fail with --if-present even if the wanted script is not present', async () => {
  prepare({})

  await run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    extraEnv: {},
    ifPresent: true,
    rawConfig: {},
  }, ['build'])
})

test('if a script is not found but is present in the root, print an info message about it in the error message', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        dependencies: {
          'json-append': '1',
        },
        scripts: {
          build: 'node -e "process.stdout.write(\'root\')" | json-append ./output.json',
        },
      },
    },
    {
      name: 'foo',
      version: '1.0.0',
    },
  ])
  await writeYamlFile('pnpm-workspace.yaml', {})

  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])

  let err!: PnpmError
  try {
    await run.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: path.resolve('foo'),
      selectedProjectsGraph,
      workspaceDir: process.cwd(),
    }, ['build'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  expect(err).toBeTruthy()
  expect(err.hint).toMatch(/But script matched with build is present in the root/)
})

test('scripts work with PnP', async () => {
  prepare({
    scripts: {
      foo: 'node -e "process.stdout.write(\'foo\')" | json-append ./output.json',
    },
  })

  await execa(pnpmBin, ['add', 'json-append@1'], {
    env: {
      NPM_CONFIG_NODE_LINKER: 'pnp',
      NPM_CONFIG_SYMLINK: 'false',
    },
  })
  await run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    extraEnv: {},
    rawConfig: {},
  }, ['foo'])

  const { default: scriptsRan } = await import(path.resolve('output.json'))
  expect(scriptsRan).toStrictEqual(['foo'])
})

test('pnpm run with custom shell', async () => {
  prepare({
    scripts: {
      build: 'foo bar',
    },
    dependencies: {
      '@pnpm.e2e/shell-mock': '0.0.0',
    },
  })

  await execa(pnpmBin, [
    'install',
    `--registry=${REGISTRY_URL}`,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])

  await run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    extraEnv: {},
    rawConfig: {},
    scriptShell: path.resolve(`node_modules/.bin/shell-mock${isWindows() ? '.cmd' : ''}`),
  }, ['build'])

  expect((await import(path.resolve('shell-input.json'))).default).toStrictEqual(['-c', 'foo bar'])
})

test('pnpm run with RegExp script selector should work', async () => {
  prepare({
    scripts: {
      'build:a': 'node -e "require(\'fs\').writeFileSync(\'./output-build-a.txt\', \'a\', \'utf8\')"',
      'build:b': 'node -e "require(\'fs\').writeFileSync(\'./output-build-b.txt\', \'b\', \'utf8\')"',
      'build:c': 'node -e "require(\'fs\').writeFileSync(\'./output-build-c.txt\', \'c\', \'utf8\')"',
      build: 'node -e "require(\'fs\').writeFileSync(\'./output-build-a.txt\', \'should not run\', \'utf8\')"',
      'lint:a': 'node -e "require(\'fs\').writeFileSync(\'./output-lint-a.txt\', \'a\', \'utf8\')"',
      'lint:b': 'node -e "require(\'fs\').writeFileSync(\'./output-lint-b.txt\', \'b\', \'utf8\')"',
      'lint:c': 'node -e "require(\'fs\').writeFileSync(\'./output-lint-c.txt\', \'c\', \'utf8\')"',
      lint: 'node -e "require(\'fs\').writeFileSync(\'./output-lint-a.txt\', \'should not run\', \'utf8\')"',
    },
  })

  await run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    extraEnv: {},
    rawConfig: {},
  }, ['/^(lint|build):.*/'])

  expect(await fs.readFile('output-build-a.txt', { encoding: 'utf-8' })).toEqual('a')
  expect(await fs.readFile('output-build-b.txt', { encoding: 'utf-8' })).toEqual('b')
  expect(await fs.readFile('output-build-c.txt', { encoding: 'utf-8' })).toEqual('c')

  expect(await fs.readFile('output-lint-a.txt', { encoding: 'utf-8' })).toEqual('a')
  expect(await fs.readFile('output-lint-b.txt', { encoding: 'utf-8' })).toEqual('b')
  expect(await fs.readFile('output-lint-c.txt', { encoding: 'utf-8' })).toEqual('c')
})

test('pnpm run with RegExp script selector should work also for pre/post script', async () => {
  prepare({
    scripts: {
      'build:a': 'node -e "require(\'fs\').writeFileSync(\'./output-a.txt\', \'a\', \'utf8\')"',
      'prebuild:a': 'node -e "require(\'fs\').writeFileSync(\'./output-pre-a.txt\', \'pre-a\', \'utf8\')"',
    },
  })

  await run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    extraEnv: {},
    rawConfig: {},
    enablePrePostScripts: true,
  }, ['/build:.*/'])

  expect(await fs.readFile('output-a.txt', { encoding: 'utf-8' })).toEqual('a')
  expect(await fs.readFile('output-pre-a.txt', { encoding: 'utf-8' })).toEqual('pre-a')
})

test('pnpm run with RegExp script selector should work parallel as a default behavior (parallel execution limits number is four)', async () => {
  prepare({
    scripts: {
      'build:a': 'node -e "let i = 20;setInterval(() => {if (!--i) process.exit(0); require(\'json-append\').append(Date.now(),\'./output-a.json\');},50)"',
      'build:b': 'node -e "let i = 40;setInterval(() => {if (!--i) process.exit(0); require(\'json-append\').append(Date.now(),\'./output-b.json\');},25)"',
    },
  })

  await execa('pnpm', ['add', 'json-append@1'])

  await run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    extraEnv: {},
    rawConfig: {},
  }, ['/build:.*/'])

  const { default: outputsA } = await import(path.resolve('output-a.json'))
  const { default: outputsB } = await import(path.resolve('output-b.json'))

  expect(Math.max(outputsA[0], outputsB[0]) < Math.min(outputsA[outputsA.length - 1], outputsB[outputsB.length - 1])).toBeTruthy()
})

test('pnpm run with RegExp script selector should work sequentially with --workspace-concurrency=1', async () => {
  prepare({
    scripts: {
      'build:a': 'node -e "let i = 2;setInterval(() => {if (!i--) process.exit(0); require(\'json-append\').append(Date.now(),\'./output-a.json\');},16)"',
      'build:b': 'node -e "let i = 2;setInterval(() => {if (!i--) process.exit(0); require(\'json-append\').append(Date.now(),\'./output-b.json\');},16)"',
    },
  })

  await execa('pnpm', ['add', 'json-append@1'])

  await run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    extraEnv: {},
    rawConfig: {},
    workspaceConcurrency: 1,
  }, ['/build:.*/'])

  const { default: outputsA } = await import(path.resolve('output-a.json'))
  const { default: outputsB } = await import(path.resolve('output-b.json'))

  expect(outputsA[0] < outputsB[0] && outputsA[1] < outputsB[1]).toBeTruthy()
})

test('pnpm run with RegExp script selector with flag should throw error', async () => {
  prepare({
    scripts: {
      'build:a': 'node -e "let i = 2;setInterval(() => {if (!i--) process.exit(0); require(\'json-append\').append(Date.now(),\'./output-a.json\');},16)"',
      'build:b': 'node -e "let i = 2;setInterval(() => {if (!i--) process.exit(0); require(\'json-append\').append(Date.now(),\'./output-b.json\');},16)"',
    },
  })

  let err!: Error
  try {
    await run.handler({
      dir: process.cwd(),
      extraBinPaths: [],
      extraEnv: {},
      rawConfig: {},
      workspaceConcurrency: 1,
    }, ['/build:.*/i'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.message).toBe('RegExp flags are not supported in script command selector')
})

test('pnpm run with slightly incorrect command suggests correct one', async () => {
  prepare({
    scripts: {
      build: 'echo 0',
    },
  })

  await expect(run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    extraEnv: {},
    rawConfig: {},
    workspaceConcurrency: 1,
  }, ['buil'])).rejects.toEqual(expect.objectContaining({
    code: 'ERR_PNPM_NO_SCRIPT',
    hint: 'Command "buil" not found. Did you mean "pnpm run build"?',
  }))
})

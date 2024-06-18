/// <reference path="../../../__typings__/index.d.ts" />
import fs from 'fs'
import path from 'path'
import { type PnpmError } from '@pnpm/error'
import { filterPackagesFromDir } from '@pnpm/workspace.filter-packages-from-dir'
import {
  restart,
  run,
  test as testCommand,
} from '@pnpm/plugin-commands-script-runners'
import { prepare, preparePackages } from '@pnpm/prepare'
import { createTestIpcServer } from '@pnpm/test-ipc-server'
import execa from 'execa'
import isWindows from 'is-windows'
import { sync as writeYamlFile } from 'write-yaml-file'
import { DEFAULT_OPTS, REGISTRY_URL } from './utils'

const pnpmBin = path.join(__dirname, '../../../pnpm/bin/pnpm.cjs')

const skipOnWindows = isWindows() ? test.skip : test
const onlyOnWindows = !isWindows() ? test.skip : test

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
  fs.writeFileSync('args.json', '[]', 'utf8')
  fs.writeFileSync('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

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
  fs.writeFileSync('args.json', '[]', 'utf8')
  fs.writeFileSync('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

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
  fs.writeFileSync('args.json', '[]', 'utf8')
  fs.writeFileSync('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

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
  fs.writeFileSync('args.json', '[]', 'utf8')
  fs.writeFileSync('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

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
  fs.writeFileSync('args.json', '[]', 'utf8')
  fs.writeFileSync('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

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
  fs.writeFileSync('args.json', '[]', 'utf8')
  fs.writeFileSync('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

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
  await using server = await createTestIpcServer()

  prepare({
    scripts: {
      poststop: server.sendLineScript('poststop'),
      prestop: server.sendLineScript('prestop'),
      stop: server.sendLineScript('stop'),

      postrestart: server.sendLineScript('postrestart'),
      prerestart: server.sendLineScript('prerestart'),
      restart: server.sendLineScript('restart'),

      poststart: server.sendLineScript('poststart'),
      prestart: server.sendLineScript('prestart'),
      start: server.sendLineScript('start'),
    },
  })

  await restart.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    extraEnv: {},
    rawConfig: {},
  }, [])

  expect(server.getLines()).toStrictEqual([
    'stop',
    'restart',
    'start',
  ])
})

test('restart: run stop, restart and start and all the pre/post scripts', async () => {
  await using server = await createTestIpcServer()

  prepare({
    scripts: {
      poststop: server.sendLineScript('poststop'),
      prestop: server.sendLineScript('prestop'),
      stop: `${server.sendLineScript('stop')} && pnpm poststop`,

      postrestart: server.sendLineScript('postrestart'),
      prerestart: server.sendLineScript('prerestart'),
      restart: server.sendLineScript('restart'),

      poststart: server.sendLineScript('poststart'),
      prestart: server.sendLineScript('prestart'),
      start: server.sendLineScript('start'),
    },
  })

  await restart.handler({
    dir: process.cwd(),
    enablePrePostScripts: true,
    extraBinPaths: [],
    extraEnv: {},
    rawConfig: {},
  }, [])

  expect(server.getLines()).toStrictEqual([
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
  writeYamlFile('pnpm-workspace.yaml', {})
  const workspaceDir = process.cwd()

  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])

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
        scripts: {
          build: 'node -e "process.stdout.write(\'root\')"',
        },
      },
    },
    {
      name: 'foo',
      version: '1.0.0',
    },
  ])
  writeYamlFile('pnpm-workspace.yaml', {})

  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  const { allProjects, selectedProjectsGraph } = await filterPackagesFromDir(process.cwd(), [])

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
      foo: 'hello-world-js-bin > ./output.txt',
    },
  })

  await execa(pnpmBin, ['add', '@pnpm.e2e/hello-world-js-bin@1.0.0'], {
    env: {
      NPM_CONFIG_REGISTRY: REGISTRY_URL,
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

  // https://github.com/pnpm/registry-mock/blob/ac2e129eb262009d2e7cd43ed869c31097793073/packages/hello-world-js-bin%401.0.0/index.js#L2
  const helloWorldJsBinOutput = 'Hello world!\n'

  const fooOutput = fs.readFileSync(path.resolve('output.txt')).toString()
  expect(fooOutput).toStrictEqual(helloWorldJsBinOutput)
})

// A .exe file to configure as the scriptShell option would be necessary to test
// this behavior on Windows. Skip this test for now since compiling a custom
// .exe would be quite involved and hard to maintain.
skipOnWindows('pnpm run with custom shell', async () => {
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
    scriptShell: path.resolve('node_modules/.bin/shell-mock'),
  }, ['build'])

  expect((await import(path.resolve('shell-input.json'))).default).toStrictEqual(['-c', 'foo bar'])
})

onlyOnWindows('pnpm shows error if script-shell is .cmd', async () => {
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

  async function runScript () {
    await run.handler({
      dir: process.cwd(),
      extraBinPaths: [],
      extraEnv: {},
      rawConfig: {},
      scriptShell: path.resolve('node_modules/.bin/shell-mock.cmd'),
    }, ['build'])
  }

  await expect(runScript).rejects.toEqual(expect.objectContaining({
    code: 'ERR_PNPM_INVALID_SCRIPT_SHELL_WINDOWS',
  }))
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

  expect(fs.readFileSync('output-build-a.txt', { encoding: 'utf-8' })).toEqual('a')
  expect(fs.readFileSync('output-build-b.txt', { encoding: 'utf-8' })).toEqual('b')
  expect(fs.readFileSync('output-build-c.txt', { encoding: 'utf-8' })).toEqual('c')

  expect(fs.readFileSync('output-lint-a.txt', { encoding: 'utf-8' })).toEqual('a')
  expect(fs.readFileSync('output-lint-b.txt', { encoding: 'utf-8' })).toEqual('b')
  expect(fs.readFileSync('output-lint-c.txt', { encoding: 'utf-8' })).toEqual('c')
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

  expect(fs.readFileSync('output-a.txt', { encoding: 'utf-8' })).toEqual('a')
  expect(fs.readFileSync('output-pre-a.txt', { encoding: 'utf-8' })).toEqual('pre-a')
})

test('pnpm run with RegExp script selector should work parallel as a default behavior (parallel execution limits number is four)', async () => {
  await using serverA = await createTestIpcServer()
  await using serverB = await createTestIpcServer()

  prepare({
    scripts: {
      'build:a': `node -e "let i = 20;setInterval(() => {if (!--i) process.exit(0); console.log(Date.now());},50)" | ${serverA.generateSendStdinScript()}`,
      'build:b': `node -e "let i = 40;setInterval(() => {if (!--i) process.exit(0); console.log(Date.now());},25)" | ${serverB.generateSendStdinScript()}`,
    },
  })

  await run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    extraEnv: {},
    rawConfig: {},
  }, ['/build:.*/'])

  const outputsA = serverA.getLines().map(x => Number.parseInt(x))
  const outputsB = serverB.getLines().map(x => Number.parseInt(x))

  expect(Math.max(outputsA[0], outputsB[0]) < Math.min(outputsA[outputsA.length - 1], outputsB[outputsB.length - 1])).toBeTruthy()
})

test('pnpm run with RegExp script selector should work sequentially with --workspace-concurrency=1', async () => {
  await using serverA = await createTestIpcServer()
  await using serverB = await createTestIpcServer()

  prepare({
    scripts: {
      'build:a': `node -e "let i = 2;setInterval(() => {if (!i--) process.exit(0); console.log(Date.now()); },16)" | ${serverA.generateSendStdinScript()}`,
      'build:b': `node -e "let i = 2;setInterval(() => {if (!i--) process.exit(0); console.log(Date.now()); },16)" | ${serverB.generateSendStdinScript()}`,
    },
  })

  await run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    extraEnv: {},
    rawConfig: {},
    workspaceConcurrency: 1,
  }, ['/build:.*/'])

  const outputsA = serverA.getLines().map(x => Number.parseInt(x))
  const outputsB = serverB.getLines().map(x => Number.parseInt(x))

  expect(outputsA[0] < outputsB[0] && outputsA[1] < outputsB[1]).toBeTruthy()
})

test('pnpm run with RegExp script selector with flag should throw error', async () => {
  await using serverA = await createTestIpcServer()
  await using serverB = await createTestIpcServer()

  prepare({
    scripts: {
      'build:a': `node -e "let i = 2;setInterval(() => {if (!i--) process.exit(0); console.log(Date.now()); },16)" | ${serverA.generateSendStdinScript()}`,
      'build:b': `node -e "let i = 2;setInterval(() => {if (!i--) process.exit(0); console.log(Date.now()); },16)" | ${serverB.generateSendStdinScript()}`,
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

  // cspell:ignore buil
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

test('pnpm run with custom node-options', async () => {
  prepare({
    scripts: {
      build: 'node -e "if (process.env.NODE_OPTIONS !== \'--max-old-space-size=1200\') { process.exit(1) }"',
    },
  })

  await run.handler({
    dir: process.cwd(),
    extraBinPaths: [],
    extraEnv: {},
    rawConfig: {},
    nodeOptions: '--max-old-space-size=1200',
    workspaceConcurrency: 1,
  }, ['build'])
})

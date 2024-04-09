import fs from 'fs'
import path from 'path'
import PATH_NAME from 'path-name'
import { createBase32Hash } from '@pnpm/crypto.base32-hash'
import { prepare, prepareEmpty, preparePackages } from '@pnpm/prepare'
import isWindows from 'is-windows'
import { execPnpm, execPnpmSync } from './utils'

const RECORD_ARGS_FILE = 'require(\'fs\').writeFileSync(\'args.json\', JSON.stringify(require(\'./args.json\').concat([process.argv.slice(2)])), \'utf8\')'
const testOnPosix = isWindows() ? test.skip : test

test('run -r: pass the args to the command that is specified in the build script', async () => {
  preparePackages([{
    name: 'project',
    scripts: {
      foo: 'node recordArgs',
      postfoo: 'node recordArgs',
      prefoo: 'node recordArgs',
    },
  }])
  fs.writeFileSync('project/args.json', '[]', 'utf8')
  fs.writeFileSync('project/recordArgs.js', RECORD_ARGS_FILE, 'utf8')

  await execPnpm(['run', '-r', '--config.enable-pre-post-scripts', 'foo', 'arg', '--flag=true'])

  const { default: args } = await import(path.resolve('project/args.json'))
  expect(args).toStrictEqual([
    [],
    ['arg', '--flag=true'],
    [],
  ])
})

test('run: pass the args to the command that is specified in the build script', async () => {
  prepare({
    name: 'project',
    scripts: {
      foo: 'node recordArgs',
      postfoo: 'node recordArgs',
      prefoo: 'node recordArgs',
    },
  })
  fs.writeFileSync('args.json', '[]', 'utf8')
  fs.writeFileSync('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

  await execPnpm(['run', 'foo', 'arg', '--flag=true'])

  const { default: args } = await import(path.resolve('args.json'))
  expect(args).toStrictEqual([
    [],
    ['arg', '--flag=true'],
    [],
  ])
})

// Before pnpm v7, `--` was required to pass flags to a build script. Now all
// arguments after the script name should be passed to the build script, even
// `--`.
test('run: pass all arguments after script name to the build script, even --', async () => {
  prepare({
    name: 'project',
    scripts: {
      foo: 'node recordArgs',
      postfoo: 'node recordArgs',
      prefoo: 'node recordArgs',
    },
  })
  fs.writeFileSync('args.json', '[]', 'utf8')
  fs.writeFileSync('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

  await execPnpm(['run', 'foo', 'arg', '--', '--flag=true'])

  const { default: args } = await import(path.resolve('args.json'))
  expect(args).toStrictEqual([
    [],
    ['arg', '--', '--flag=true'],
    [],
  ])
})

test('exit code of child process is preserved', async () => {
  prepare({
    scripts: {
      foo: 'exit 87',
    },
  })
  const result = execPnpmSync(['run', 'foo'])
  expect(result.status).toBe(87)
})

test('test -r: pass the args to the command that is specified in the build script of a package.json manifest', async () => {
  preparePackages([{
    name: 'project',
    scripts: {
      test: 'ts-node test',
    },
  }])

  const result = execPnpmSync(['test', '-r', 'arg', '--', '--flag=true'])

  expect((result.stdout as Buffer).toString('utf8')).toMatch(/ts-node test "arg" "--flag=true"/)
})

test('start: run "node server.js" by default', async () => {
  prepare({}, { manifestFormat: 'YAML' })

  fs.writeFileSync('server.js', 'console.log("Hello world!")', 'utf8')

  const result = execPnpmSync(['start'])

  expect((result.stdout as Buffer).toString('utf8')).toMatch(/Hello world!/)
})

test('install-test: install dependencies and runs tests', async () => {
  prepare({
    scripts: {
      test: 'node -e "process.stdout.write(\'test\')" > ./output.txt',
    },
  }, { manifestFormat: 'JSON5' })

  await execPnpm(['install-test'])

  const scriptsRan = (fs.readFileSync('output.txt')).toString()
  expect(scriptsRan.trim()).toStrictEqual('test')
})

test('silent run only prints the output of the child process', async () => {
  prepare({
    scripts: {
      hi: 'echo hi && exit 1',
    },
  })

  const result = execPnpmSync(['run', '--silent', 'hi'])

  expect(result.stdout.toString().trim()).toBe('hi')
})

test('silent dlx prints the output of the child process only', async () => {
  prepare({})
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(global)

  const env = {
    [PATH_NAME]: `${pnpmHome}${path.delimiter}${process.env[PATH_NAME]}`,
    PNPM_HOME: pnpmHome,
    XDG_DATA_HOME: global,
  }

  const result = execPnpmSync(['--silent', 'dlx', 'shx', 'echo', 'hi'], { env })

  expect(result.stdout.toString().trim()).toBe('hi')
})

test('dlx ignores configuration in current project package.json', async () => {
  prepare({
    pnpm: {
      patchedDependencies: {
        'shx@0.3.4': 'this_does_not_exist',
      },
    },
  })
  const global = path.resolve('..', 'global')
  const pnpmHome = path.join(global, 'pnpm')
  fs.mkdirSync(global)

  const env = {
    [PATH_NAME]: `${pnpmHome}${path.delimiter}${process.env[PATH_NAME]}`,
    PNPM_HOME: pnpmHome,
    XDG_DATA_HOME: global,
  }

  const result = execPnpmSync(['dlx', 'shx@0.3.4', 'echo', 'hi'], { env })
  // It didn't try to use the patch that doesn't exist, so it did not fail
  expect(result.status).toBe(0)
})

testOnPosix('pnpm run with preferSymlinkedExecutables true', async () => {
  prepare({
    scripts: {
      build: 'node -e "console.log(process.env.NODE_PATH)"',
    },
  })

  const npmrc = `
    prefer-symlinked-executables=true=true
  `

  fs.writeFileSync('.npmrc', npmrc, 'utf8')

  const result = execPnpmSync(['run', 'build'])

  expect(result.stdout.toString()).toContain(`project${path.sep}node_modules${path.sep}.pnpm${path.sep}node_modules`)
})

testOnPosix('pnpm run with preferSymlinkedExecutables and custom virtualStoreDir', async () => {
  prepare({
    scripts: {
      build: 'node -e "console.log(process.env.NODE_PATH)"',
    },
  })

  const npmrc = `
    virtual-store-dir=/foo/bar
    prefer-symlinked-executables=true=true
  `

  fs.writeFileSync('.npmrc', npmrc, 'utf8')

  const result = execPnpmSync(['run', 'build'])

  expect(result.stdout.toString()).toContain(`${path.sep}foo${path.sep}bar${path.sep}node_modules`)
})

test('collapse output when running multiple scripts in one project', async () => {
  prepare({
    scripts: {
      script1: 'echo 1',
      script2: 'echo 2',
    },
  })

  const result = execPnpmSync(['run', '/script[12]/'])

  const output = result.stdout.toString()
  expect(output).toContain('script1: 1')
  expect(output).toContain('script2: 2')
})

test('do not collapse output when running multiple scripts in one project sequentially', async () => {
  prepare({
    scripts: {
      script1: 'echo 1',
      script2: 'echo 2',
    },
  })

  const result = execPnpmSync(['--workspace-concurrency=1', 'run', '/script[12]/'])

  const output = result.stdout.toString()
  expect(output).not.toContain('script1: 1')
  expect(output).not.toContain('script2: 2')
})

test('--parallel should work with single project', async () => {
  prepare({
    scripts: {
      script1: 'echo 1',
      script2: 'echo 2',
    },
  })

  const result = execPnpmSync(['--parallel', 'run', '/script[12]/'])

  const output = result.stdout.toString()
  expect(output).toContain('script1: 1')
  expect(output).toContain('script2: 2')
})

test('--reporter-hide-prefix should hide workspace prefix', async () => {
  prepare({
    scripts: {
      script1: 'echo 1',
      script2: 'echo 2',
    },
  })

  const result = execPnpmSync(['--parallel', '--reporter-hide-prefix', 'run', '/script[12]/'])

  const output = result.stdout.toString()
  expect(output).toContain('1')
  expect(output).not.toContain('script1: 1')
  expect(output).toContain('2')
  expect(output).not.toContain('script2: 2')
})

test('dlx should work with npm_config_save_dev env variable', async () => {
  prepareEmpty()
  const result = execPnpmSync(['dlx', '@foo/touch-file-one-bin@latest'], {
    env: {
      npm_config_save_dev: 'true',
    },
    stdio: 'inherit',
  })
  expect(result.status).toBe(0)
})

test('parallel dlx calls of the same package', async () => {
  prepareEmpty()

  // parallel dlx calls without cache
  await Promise.all(['foo', 'bar', 'baz'].map(
    name => execPnpm([
      `--config.store-dir=${path.resolve('store')}`,
      `--config.cache-dir=${path.resolve('cache')}`,
      '--config.dlx-cache-max-age=Infinity',
      'dlx', 'shx', 'touch', name])
  ))

  expect(['foo', 'bar', 'baz'].filter(name => fs.existsSync(name))).toStrictEqual(['foo', 'bar', 'baz'])
  expect(fs.readdirSync(path.resolve('cache', 'dlx', createBase32Hash('shx'))).length).toBe(4)
  expect(
    fs.readdirSync(path.resolve('cache', 'dlx', createBase32Hash('shx'), 'pkg'))
  ).toStrictEqual([
    'node_modules',
    'package.json',
    'pnpm-lock.yaml',
  ])
  expect(
    path.dirname(fs.realpathSync(path.resolve('cache', 'dlx', createBase32Hash('shx'), 'pkg')))
  ).toBe(path.resolve('cache', 'dlx', createBase32Hash('shx')))

  // parallel dlx calls with cache
  await Promise.all(['abc', 'def', 'ghi'].map(
    name => execPnpm(['dlx', 'shx', 'mkdir', name])
  ))

  expect(['abc', 'def', 'ghi'].filter(name => fs.existsSync(name))).toStrictEqual(['abc', 'def', 'ghi'])
  expect(fs.readdirSync(path.resolve('cache', 'dlx', createBase32Hash('shx'))).length).toBe(4)
  expect(
    fs.readdirSync(path.resolve('cache', 'dlx', createBase32Hash('shx'), 'pkg'))
  ).toStrictEqual([
    'node_modules',
    'package.json',
    'pnpm-lock.yaml',
  ])
  expect(
    path.dirname(fs.realpathSync(path.resolve('cache', 'dlx', createBase32Hash('shx'), 'pkg')))
  ).toBe(path.resolve('cache', 'dlx', createBase32Hash('shx')))

  // parallel dlx calls with expired cache
  await Promise.all(['a/b/c', 'd/e/f', 'g/h/i'].map(
    dirPath => execPnpm([
      `--config.store-dir=${path.resolve('store')}`,
      `--config.cache-dir=${path.resolve('cache')}`,
      '--config.dlx-cache-max-age=0',
      'dlx', 'shx', 'mkdir', '-p', dirPath])
  ))

  expect(['a/b/c', 'd/e/f', 'g/h/i'].filter(name => fs.existsSync(name))).toStrictEqual(['a/b/c', 'd/e/f', 'g/h/i'])
  expect(fs.readdirSync(path.resolve('cache', 'dlx', createBase32Hash('shx'))).length).toBe(7)
  expect(
    fs.readdirSync(path.resolve('cache', 'dlx', createBase32Hash('shx'), 'pkg'))
  ).toStrictEqual([
    'node_modules',
    'package.json',
    'pnpm-lock.yaml',
  ])
  expect(
    path.dirname(fs.realpathSync(path.resolve('cache', 'dlx', createBase32Hash('shx'), 'pkg')))
  ).toBe(path.resolve('cache', 'dlx', createBase32Hash('shx')))
})

test('dlx creates cache and store prune cleans cache', async () => {
  prepareEmpty()

  const commands = {
    shx: ['echo', 'hello from shx'],
    'shelljs/shx#61aca968cd7afc712ca61a4fc4ec3201e3770dc7': ['echo', 'hello from shx.git'],
    '@pnpm.e2e/touch-file-good-bin-name': [],
    '@pnpm.e2e/touch-file-one-bin': [],
  } satisfies Record<string, string[]>

  const settings = [
    `--config.store-dir=${path.resolve('store')}`,
    `--config.cache-dir=${path.resolve('cache')}`,
    '--config.dlx-cache-max-age=50', // big number to avoid false negative should test unexpectedly takes too long to run
  ]

  await Promise.all(Object.entries(commands).map(([cmd, args]) => execPnpm([...settings, 'dlx', cmd, ...args])))

  // ensure that the dlx cache has certain structure
  expect(
    fs.readdirSync(path.resolve('cache', 'dlx'))
      .sort()
  ).toStrictEqual(
    Object.keys(commands)
      .map(createBase32Hash)
      .sort()
  )
  for (const cmd of Object.keys(commands)) {
    expect(fs.readdirSync(path.resolve('cache', 'dlx', createBase32Hash(cmd))).length).toBe(2)
  }

  // modify the dates of the cache items
  const ageTable = {
    shx: 20,
    'shelljs/shx#61aca968cd7afc712ca61a4fc4ec3201e3770dc7': 75,
    '@pnpm.e2e/touch-file-good-bin-name': 33,
    '@pnpm.e2e/touch-file-one-bin': 123,
  } satisfies Record<keyof typeof commands, number>
  const now = new Date()
  await Promise.all(Object.entries(ageTable).map(async ([cmd, age]) => {
    const newDate = new Date(now.getTime() - age * 60_000)
    const dlxCacheLink = path.resolve('cache', 'dlx', createBase32Hash(cmd), 'pkg')
    await fs.promises.lutimes(dlxCacheLink, newDate, newDate)
  }))

  await execPnpm([...settings, 'store', 'prune'])

  // test to see if dlx cache items are deleted or kept as expected
  expect(
    fs.readdirSync(path.resolve('cache', 'dlx'))
      .sort()
  ).toStrictEqual(
    ['shx', '@pnpm.e2e/touch-file-good-bin-name']
      .map(createBase32Hash)
      .sort()
  )
  for (const cmd of ['shx', '@pnpm.e2e/touch-file-good-bin-name']) {
    expect(fs.readdirSync(path.resolve('cache', 'dlx', createBase32Hash(cmd))).length).toBe(2)
  }

  await execPnpm([
    `--config.store-dir=${path.resolve('store')}`,
    `--config.cache-dir=${path.resolve('cache')}`,
    '--config.dlx-cache-max-age=0',
    'store', 'prune'])

  // test to see if all dlx cache items are deleted
  expect(
    fs.readdirSync(path.resolve('cache', 'dlx'))
      .sort()
  ).toStrictEqual([])
})

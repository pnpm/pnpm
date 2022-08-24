import { promises as fs, mkdirSync } from 'fs'
import path from 'path'
import PATH_NAME from 'path-name'
import prepare, { preparePackages } from '@pnpm/prepare'
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
  await fs.writeFile('project/args.json', '[]', 'utf8')
  await fs.writeFile('project/recordArgs.js', RECORD_ARGS_FILE, 'utf8')

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
  await fs.writeFile('args.json', '[]', 'utf8')
  await fs.writeFile('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

  await execPnpm(['run', 'foo', 'arg', '--flag=true'])

  const { default: args } = await import(path.resolve('args.json'))
  expect(args).toStrictEqual([
    ['arg', '--flag=true'],
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
  await fs.writeFile('args.json', '[]', 'utf8')
  await fs.writeFile('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

  await execPnpm(['run', 'foo', 'arg', '--', '--flag=true'])

  const { default: args } = await import(path.resolve('args.json'))
  expect(args).toStrictEqual([
    ['arg', '--', '--flag=true'],
  ])
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

  await fs.writeFile('server.js', 'console.log("Hello world!")', 'utf8')

  const result = execPnpmSync(['start'])

  expect((result.stdout as Buffer).toString('utf8')).toMatch(/Hello world!/)
})

test('install-test: install dependencies and runs tests', async () => {
  prepare({
    dependencies: {
      'json-append': '1',
    },
    scripts: {
      test: 'node -e "process.stdout.write(\'test\')" | json-append ./output.json',
    },
  }, { manifestFormat: 'JSON5' })

  await execPnpm(['install-test'])

  const { default: scriptsRan } = await import(path.resolve('output.json'))
  expect(scriptsRan).toStrictEqual(['test'])
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
  mkdirSync(global)

  const env = {
    [PATH_NAME]: `${pnpmHome}${path.delimiter}${process.env[PATH_NAME]}`, // eslint-disable-line
    PNPM_HOME: pnpmHome,
    XDG_DATA_HOME: global,
  }

  const result = execPnpmSync(['--silent', 'dlx', 'shx', 'echo', 'hi'], { env })

  expect(result.stdout.toString().trim()).toBe('hi')
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

  await fs.writeFile('.npmrc', npmrc, 'utf8')

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

  await fs.writeFile('.npmrc', npmrc, 'utf8')

  const result = execPnpmSync(['run', 'build'])

  expect(result.stdout.toString()).toContain(`${path.sep}foo${path.sep}bar${path.sep}node_modules`)
})

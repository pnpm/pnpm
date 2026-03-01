import fs from 'fs'
import path from 'path'
import { prepare, preparePackages } from '@pnpm/prepare'
import isWindows from 'is-windows'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpm, execPnpmSync } from './utils/index.js'

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

  await execPnpm(['run', '-r', '--config.enable-pre-post-scripts', '--config.verify-deps-before-run=false', 'foo', 'arg', '--flag=true'])

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

test('recursive test: pass the args to the command that is specified in the build script of a package.json manifest', async () => {
  preparePackages([{
    name: 'project',
    scripts: {
      test: 'ts-node test',
    },
  }])

  const result = execPnpmSync(['--config.verify-deps-before-run=false', '-r', 'test', 'arg', '--flag=true'])

  expect((result.stdout as Buffer).toString('utf8')).toMatch(
    process.platform === 'win32' ? /ts-node test "arg" "--flag=true"/ : /ts-node test arg --flag=true/
  )
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
  expect(scriptsRan.trim()).toBe('test')
})

test('silent run only prints the output of the child process', async () => {
  prepare({
    scripts: {
      hi: 'echo hi && exit 1',
    },
  })

  const result = execPnpmSync(['run', '--silent', '--config.verify-deps-before-run=false', 'hi'])

  expect(result.stdout.toString().trim()).toBe('hi')
})

testOnPosix('pnpm run with preferSymlinkedExecutables true', async () => {
  prepare({
    scripts: {
      build: 'node -e "console.log(process.env.NODE_PATH)"',
    },
  })

  writeYamlFile('pnpm-workspace.yaml', {
    preferSymlinkedExecutables: true,
  })

  const result = execPnpmSync(['run', 'build'])

  expect(result.stdout.toString()).toContain(`project${path.sep}node_modules${path.sep}.pnpm${path.sep}node_modules`)
})

testOnPosix('pnpm run with preferSymlinkedExecutables and custom virtualStoreDir', async () => {
  prepare({
    scripts: {
      build: 'node -e "console.log(process.env.NODE_PATH)"',
    },
  })

  writeYamlFile('pnpm-workspace.yaml', {
    virtualStoreDir: '/foo/bar',
    preferSymlinkedExecutables: true,
  })

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

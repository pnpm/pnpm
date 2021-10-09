import { promises as fs } from 'fs'
import path from 'path'
import prepare, { preparePackages } from '@pnpm/prepare'
import { execPnpm, execPnpmSync } from './utils'

const RECORD_ARGS_FILE = 'require(\'fs\').writeFileSync(\'args.json\', JSON.stringify(require(\'./args.json\').concat([process.argv.slice(2)])), \'utf8\')'

test('run -r: pass the args to the command that is specfied in the build script', async () => {
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

  await execPnpm(['run', '-r', 'foo', 'arg', '--', '--flag=true'])

  const { default: args } = await import(path.resolve('project/args.json'))
  expect(args).toStrictEqual([
    [],
    ['arg', '--flag=true'],
    [],
  ])
})

test('test -r: pass the args to the command that is specfied in the build script of a package.json manifest', async () => {
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

  const result = execPnpmSync(['--silent', 'dlx', 'shx', 'echo', 'hi'])

  expect(result.stdout.toString().trim()).toBe('hi')
})

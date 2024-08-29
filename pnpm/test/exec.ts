import { prepare, preparePackages } from '@pnpm/prepare'
import { execPnpmSync } from './utils'

test('exec with executionEnv', async () => {
  prepare({
    name: 'test',
    version: '0.0.0',
    pnpm: {
      executionEnv: {
        nodeVersion: '18.0.0',
      },
    },
  })

  const output = execPnpmSync(['exec', 'node', '--version']).stdout.toString().trim()
  expect(output).toStrictEqual('v18.0.0')
})

test('recursive exec when some packages define different executionEnv', async () => {
  preparePackages([
    {
      name: 'node-version-unset',
      version: '0.0.0',
    },
    {
      name: 'node-version-18',
      version: '0.0.0',
      pnpm: {
        executionEnv: {
          nodeVersion: '18.0.0',
        },
      },
    },
    {
      name: 'node-version-20',
      version: '0.0.0',
      pnpm: {
        executionEnv: {
          nodeVersion: '20.0.0',
        },
      },
    },
  ])

  const execNodePrintVersion = (extraOptions: string[]) =>
    execPnpmSync([
      ...extraOptions,
      '--recursive',
      '--reporter-hide-prefix',
      'exec',
      'node',
      '--print',
      '">>> " + require("./package.json").name + ": " + process.version',
    ])
      .stdout
      .toString()
      .trim()
      .split('\n')
      .filter(x => x.startsWith('>>> '))
      .sort()

  expect(execNodePrintVersion([])).toStrictEqual([
    '>>> node-version-18: v18.0.0',
    '>>> node-version-20: v20.0.0',
    `>>> node-version-unset: ${process.version}`,
  ])

  expect(execNodePrintVersion(['--use-node-version=19.0.0'])).toStrictEqual([
    '>>> node-version-18: v18.0.0',
    '>>> node-version-20: v20.0.0',
    '>>> node-version-unset: v19.0.0',
  ])
})

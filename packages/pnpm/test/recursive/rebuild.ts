import { preparePackages } from '@pnpm/prepare'
import promisifyTape from 'tape-promise'
import { execPnpm } from '../utils'
import tape = require('tape')

const test = promisifyTape(tape)

test('`pnpm recursive rebuild` specific dependencies', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'install-scripts-example-for-pnpm': 'zkochan/install-scripts-example',
        'pre-and-postinstall-scripts-example': '*',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'install-scripts-example-for-pnpm': 'zkochan/install-scripts-example',
        'pre-and-postinstall-scripts-example': '*',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
    },
  ])

  await execPnpm(['recursive', 'install', '--ignore-scripts'])

  await projects['project-1'].hasNot('pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-1'].hasNot('pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  await projects['project-2'].hasNot('pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-2'].hasNot('pre-and-postinstall-scripts-example/generated-by-postinstall.js')

  await execPnpm(['recursive', 'rebuild', 'install-scripts-example-for-pnpm'])

  await projects['project-1'].hasNot('pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-1'].hasNot('pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  await projects['project-2'].hasNot('pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  await projects['project-2'].hasNot('pre-and-postinstall-scripts-example/generated-by-postinstall.js')

  {
    const generatedByPreinstall = projects['project-1'].requireModule('install-scripts-example-for-pnpm/generated-by-preinstall')
    t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

    const generatedByPostinstall = projects['project-1'].requireModule('install-scripts-example-for-pnpm/generated-by-postinstall')
    t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
  }

  {
    const generatedByPreinstall = projects['project-2'].requireModule('install-scripts-example-for-pnpm/generated-by-preinstall')
    t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

    const generatedByPostinstall = projects['project-2'].requireModule('install-scripts-example-for-pnpm/generated-by-postinstall')
    t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
  }
})

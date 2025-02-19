import { preparePackages } from '@pnpm/prepare'
import { execPnpm } from '../utils'

test('`pnpm recursive rebuild` specific dependencies', async () => {
  const projects = preparePackages([
    {
      location: '.',
      package: {
        pnpm: {
          neverBuiltDependencies: [],
        },
      },
    },
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'install-scripts-example-for-pnpm': 'pnpm-e2e/install-scripts-example#b6cfdb8af6f8d5ebc5e7de6831af9d38084d765b',
        '@pnpm.e2e/pre-and-postinstall-scripts-example': '*',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'install-scripts-example-for-pnpm': 'pnpm-e2e/install-scripts-example#b6cfdb8af6f8d5ebc5e7de6831af9d38084d765b',
        '@pnpm.e2e/pre-and-postinstall-scripts-example': '*',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
    },
  ])

  await execPnpm(['recursive', 'install', '--ignore-scripts'])

  projects['project-1'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  projects['project-1'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  projects['project-2'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  projects['project-2'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')

  await execPnpm(['recursive', 'rebuild', 'install-scripts-example-for-pnpm'])

  projects['project-1'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  projects['project-1'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')
  projects['project-2'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-preinstall.js')
  projects['project-2'].hasNot('@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js')

  {
    const generatedByPreinstall = projects['project-1'].requireModule('install-scripts-example-for-pnpm/generated-by-preinstall')
    expect(typeof generatedByPreinstall).toBe('function') // generatedByPreinstall() is available

    const generatedByPostinstall = projects['project-1'].requireModule('install-scripts-example-for-pnpm/generated-by-postinstall')
    expect(typeof generatedByPostinstall).toBe('function') // generatedByPostinstall() is available
  }

  {
    const generatedByPreinstall = projects['project-2'].requireModule('install-scripts-example-for-pnpm/generated-by-preinstall')
    expect(typeof generatedByPreinstall).toBe('function') // generatedByPreinstall() is available

    const generatedByPostinstall = projects['project-2'].requireModule('install-scripts-example-for-pnpm/generated-by-postinstall')
    expect(typeof generatedByPostinstall).toBe('function') // generatedByPostinstall() is available
  }
})

import prepare, { preparePackages } from '@pnpm/prepare'
import fs = require('mz/fs')
import path = require('path')
import exists = require('path-exists')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import writeYamlFile = require('write-yaml-file')
import { execPnpm, execPnpmSync } from './utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

const CREDENTIALS = [
  '--//localhost:4873/:username=username',
  `--//localhost:4873/:_password=${Buffer.from('password').toString('base64')}`,
  '--//localhost:4873/:email=foo@bar.net',
]

test('publish: package with package.json', async (t: tape.Test) => {
  prepare(t, {
    name: 'test-publish-package.json',
    version: '0.0.0',
  })

  await execPnpm('publish', ...CREDENTIALS)
})

test('publish: package with package.yaml', async (t: tape.Test) => {
  prepare(t, {
    name: 'test-publish-package.yaml',
    version: '0.0.0',
  }, { manifestFormat: 'YAML' })

  await execPnpm('publish', ...CREDENTIALS)

  t.ok(await exists('package.yaml'))
  t.notOk(await exists('package.json'))
})

test('publish: package with package.json5', async (t: tape.Test) => {
  prepare(t, {
    name: 'test-publish-package.json5',
    version: '0.0.0',
  }, { manifestFormat: 'JSON5' })

  await execPnpm('publish', ...CREDENTIALS)

  t.ok(await exists('package.json5'))
  t.notOk(await exists('package.json'))
})

test('publish: package with package.json5 running publish from different folder', async (t: tape.Test) => {
  prepare(t, {
    name: 'test-publish-package.json5',
    version: '0.0.1',
  }, { manifestFormat: 'JSON5' })

  process.chdir('..')

  await execPnpm('publish', 'project', ...CREDENTIALS)

  t.ok(await exists('project/package.json5'))
  t.notOk(await exists('project/package.json'))
})

test('pack packages with workspace LICENSE if no own LICENSE is present', async (t: tape.Test) => {
  preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '1.0.0',
    },
    {
      name: 'target',
      version: '1.0.0',
    },
  ], { manifestFormat: 'YAML' })

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await fs.writeFile('LICENSE', 'workspace license', 'utf8')
  await fs.writeFile('project-2/LICENSE', 'project-2 license', 'utf8')

  process.chdir('project-1')
  await execPnpm('pack')

  process.chdir('../project-2')
  await execPnpm('pack')

  process.chdir('../target')

  await execPnpm('add', '../project-1/project-1-1.0.0.tgz', '../project-2/project-2-1.0.0.tgz')

  t.equal(await fs.readFile('node_modules/project-1/LICENSE', 'utf8'), 'workspace license')
  t.equal(await fs.readFile('node_modules/project-2/LICENSE', 'utf8'), 'project-2 license')

  process.chdir('..')
  t.notOk(await exists('project-1/LICENSE'))
  t.ok(await exists('project-2/LICENSE'))
})

test('publish packages with workspace LICENSE if no own LICENSE is present', async (t: tape.Test) => {
  preparePackages(t, [
    {
      name: 'project-100',
      version: '1.0.0',
    },
    {
      name: 'project-200',
      version: '1.0.0',
    },
    {
      name: 'target',
      version: '1.0.0',
    },
  ], { manifestFormat: 'YAML' })

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await fs.writeFile('LICENSE', 'workspace license', 'utf8')
  await fs.writeFile('project-200/LICENSE', 'project-200 license', 'utf8')

  process.chdir('project-100')
  await execPnpm('publish', ...CREDENTIALS)

  process.chdir('../project-200')
  await execPnpm('publish', ...CREDENTIALS)

  process.chdir('../target')

  await execPnpm('add', 'project-100', 'project-200', '--no-link-workspace-packages')

  t.equal(await fs.readFile('node_modules/project-100/LICENSE', 'utf8'), 'workspace license')
  t.equal(await fs.readFile('node_modules/project-200/LICENSE', 'utf8'), 'project-200 license')

  process.chdir('..')
  t.notOk(await exists('project-100/LICENSE'))
  t.ok(await exists('project-200/LICENSE'))
})

test('publish: package with main, module, typings and types in publishConfig', async (t: tape.Test) => {
  preparePackages(t, [
    {
      name: 'test-publish-config',
      version: '1.0.0',

      main: './index.js',
      module: './index.mjs',
      types: `./types.d.ts`,
      typings: `./typings.d.ts`,

      publishConfig: {
        main: './published.js',
        module: './published.mjs',
        types: `./published-types.d.ts`,
        typings: `./published-typings.d.ts`,
      },
    },
    {
      name: 'test-publish-config-installation',
      version: '1.0.0',
    },
  ])

  process.chdir('test-publish-config')
  await execPnpm('publish', ...CREDENTIALS)

  const originalManifests = await import(path.resolve('package.json'))
  t.deepEqual(originalManifests, {
    name: 'test-publish-config',
    version: '1.0.0',

    main: './index.js',
    module: './index.mjs',
    types: `./types.d.ts`,
    typings: `./typings.d.ts`,

    publishConfig: {
      main: './published.js',
      module: './published.mjs',
      types: `./published-types.d.ts`,
      typings: `./published-typings.d.ts`,
    },
  })

  process.chdir('../test-publish-config-installation')
  await execPnpm('add', 'test-publish-config')

  const publishedManifest = await import(path.resolve('node_modules/test-publish-config/package.json'))
  t.deepEqual(publishedManifest, {
    name: 'test-publish-config',
    version: '1.0.0',

    main: './published.js',
    module: './published.mjs',
    types: `./published-types.d.ts`,
    typings: `./published-typings.d.ts`,

    publishConfig: {
      main: './published.js',
      module: './published.mjs',
      types: `./published-types.d.ts`,
      typings: `./published-typings.d.ts`,
    },
  })
})

test['skip']('publish package that calls executable from the workspace .bin folder in prepublishOnly script', async (t: tape.Test) => {
  preparePackages(t, [
    {
      location: '.',
      package: {
        name: 'project-100',
        version: '1.0.0',

        dependencies: {
          'json-append': '1',
        },
      },
    },
    {
      name: 'test-publish-scripts',
      version: '1.0.0',

      scripts: {
        prepublish: `node -e "process.stdout.write('prepublish')" | json-append ./output.json`,

        prepare: `node -e "process.stdout.write('prepare')" | json-append ./output.json`,

        prepublishOnly: `node -e "process.stdout.write('prepublishOnly')" | json-append ./output.json`,

        prepack: `node -e "process.stdout.write('prepack')" | json-append ./output.json`,

        postpack: `node -e "process.stdout.write('postpack')" | json-append ./output.json`,

        publish: `node -e "process.stdout.write('publish')" | json-append ./output.json`,

        postpublish: `node -e "process.stdout.write('postpublish')" | json-append ./output.json`,
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  process.chdir('test-publish-scripts')
  await execPnpm('publish', ...CREDENTIALS)

  t.deepEqual(
    await import(path.resolve('output.json')),
    [
      'prepublish',
      'prepare',
      'prepublishOnly',
      'prepack',
      'postpack',
      'publish',
      'postpublish',
    ],
  )
})

test('convert specs with workspace protocols to regular version ranges', async (t: tape.Test) => {
  preparePackages(t, [
    {
      name: 'workspace-protocol-package',
      version: '1.0.0',

      dependencies: {
        'file-type': 'workspace:12.0.1',
        'is-negative': 'workspace:*',
        'is-positive': '1.0.0',
        'lodash.delay': '~4.1.0',
      },
      optionalDependencies: {
        'lodash.deburr': 'workspace:^4.1.0',
      }
    },
    {
      name: 'is-negative',
      version: '1.0.0',
    },
    {
      name: 'file-type',
      version: '12.0.1',
    },
    {
      name: 'lodash.deburr',
      version: '4.1.0',
    },
    {
      name: 'lodash.delay',
      version: '4.1.0',
    },
    {
      name: 'target',
      version: '1.0.0',
    }
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  process.chdir('workspace-protocol-package')

  const { status, stdout } = execPnpmSync('publish', ...CREDENTIALS)

  t.equal(status, 1, 'publish fails if cannot resolve workspace:*')
  t.ok(
    stdout.toString().includes('Cannot resolve workspace protocol of dependency "is-negative"'),
    'publish fails with the correct error message',
  )

  process.chdir('..')

  await execPnpm('multi', 'install', '--store', 'store')

  process.chdir('workspace-protocol-package')
  await execPnpm('publish', ...CREDENTIALS)

  process.chdir('../target')

  await execPnpm('add', 'workspace-protocol-package', '--no-link-workspace-packages')

  const publishedManifest = await import(path.resolve('node_modules/workspace-protocol-package/package.json'))
  t.deepEqual(publishedManifest.dependencies, {
    'file-type': '12.0.1',
    'is-negative': '1.0.0',
    'is-positive': '1.0.0',
    'lodash.delay': '~4.1.0',
  })
  t.deepEqual(publishedManifest.optionalDependencies, {
    'lodash.deburr': '^4.1.0',
  })
})

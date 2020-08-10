import PnpmError from '@pnpm/error'
import { pack, publish } from '@pnpm/plugin-commands-publishing'
import prepare, { preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import crossSpawn = require('cross-spawn')
import fs = require('mz/fs')
import path = require('path')
import exists = require('path-exists')
import test = require('tape')
import writeYamlFile = require('write-yaml-file')
import { DEFAULT_OPTS } from './utils'

const CREDENTIALS = [
  `--registry=http://localhost:${REGISTRY_MOCK_PORT}/`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:username=username`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:_password=${Buffer.from('password').toString('base64')}`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:email=foo@bar.net`,
]
const pnpmBin = path.join(__dirname, '../../pnpm/bin/pnpm.js')

test('publish: package with package.json', async (t) => {
  prepare(t, {
    name: 'test-publish-package.json',
    version: '0.0.0',
  })

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', ...CREDENTIALS] },
    dir: process.cwd(),
  }, [])
  t.end()
})

test('publish: package with package.yaml', async (t) => {
  prepare(t, {
    name: 'test-publish-package.yaml',
    version: '0.0.0',
  }, { manifestFormat: 'YAML' })

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', ...CREDENTIALS] },
    dir: process.cwd(),
  }, [])

  t.ok(await exists('package.yaml'))
  t.notOk(await exists('package.json'))
  t.end()
})

test('publish: package with package.json5', async (t) => {
  prepare(t, {
    name: 'test-publish-package.json5',
    version: '0.0.0',
  }, { manifestFormat: 'JSON5' })

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', ...CREDENTIALS] },
    dir: process.cwd(),
  }, [])

  t.ok(await exists('package.json5'))
  t.notOk(await exists('package.json'))
  t.end()
})

test('publish: package with package.json5 running publish from different folder', async (t) => {
  prepare(t, {
    name: 'test-publish-package.json5',
    version: '0.0.1',
  }, { manifestFormat: 'JSON5' })

  process.chdir('..')

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', ...CREDENTIALS, 'project'] },
    dir: process.cwd(),
  }, ['project'])

  t.ok(await exists('project/package.json5'))
  t.notOk(await exists('project/package.json'))
  t.end()
})

test('pack packages with workspace LICENSE if no own LICENSE is present', async (t) => {
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

  const workspaceDir = process.cwd()
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await fs.writeFile('LICENSE', 'workspace license', 'utf8')
  await fs.writeFile('project-2/LICENSE', 'project-2 license', 'utf8')

  process.chdir('project-1')
  await pack.handler({ argv: { original: [] }, dir: process.cwd(), workspaceDir })

  process.chdir('../project-2')
  await pack.handler({ argv: { original: [] }, dir: process.cwd(), workspaceDir })

  process.chdir('../target')

  crossSpawn.sync(pnpmBin, ['add', '../project-1/project-1-1.0.0.tgz', '../project-2/project-2-1.0.0.tgz'])

  t.equal(await fs.readFile('node_modules/project-1/LICENSE', 'utf8'), 'workspace license')
  t.equal(await fs.readFile('node_modules/project-2/LICENSE', 'utf8'), 'project-2 license')

  process.chdir('..')
  t.notOk(await exists('project-1/LICENSE'))
  t.ok(await exists('project-2/LICENSE'))
  t.end()
})

test('publish packages with workspace LICENSE if no own LICENSE is present', async (t) => {
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

  const workspaceDir = process.cwd()
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await fs.writeFile('LICENSE', 'workspace license', 'utf8')
  await fs.writeFile('project-200/LICENSE', 'project-200 license', 'utf8')

  process.chdir('project-100')
  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', ...CREDENTIALS] },
    dir: process.cwd(),
    workspaceDir,
  }, [])

  process.chdir('../project-200')
  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', ...CREDENTIALS] },
    dir: process.cwd(),
    workspaceDir,
  }, [])

  process.chdir('../target')

  crossSpawn.sync(pnpmBin, ['add', 'project-100', 'project-200', '--no-link-workspace-packages', `--registry=http://localhost:${REGISTRY_MOCK_PORT}`])

  t.equal(await fs.readFile('node_modules/project-100/LICENSE', 'utf8'), 'workspace license')
  t.equal(await fs.readFile('node_modules/project-200/LICENSE', 'utf8'), 'project-200 license')

  process.chdir('..')
  t.notOk(await exists('project-100/LICENSE'))
  t.ok(await exists('project-200/LICENSE'))
  t.end()
})

test('publish: package with all possible fields in publishConfig', async (t) => {
  preparePackages(t, [
    {
      name: 'test-publish-config',
      version: '1.0.0',

      bin: './bin.js',
      main: './index.js',
      module: './index.mjs',
      types: `./types.d.ts`,
      typings: `./typings.d.ts`,

      publishConfig: {
        bin: './published-bin.js',
        browser: './published-browser.js',
        es2015: './published-es2015.js',
        esnext: './published-esnext.js',
        exports: './published-exports.js',
        main: './published.js',
        module: './published.mjs',
        types: `./published-types.d.ts`,
        typings: `./published-typings.d.ts`,
        'umd:main': './published-umd.js',
        unpkg: './published-unpkg.js',
      },
    },
    {
      name: 'test-publish-config-installation',
      version: '1.0.0',
    },
  ])

  process.chdir('test-publish-config')
  await fs.writeFile('published-bin.js', `#!/usr/bin/env node`, 'utf8')
  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', ...CREDENTIALS] },
    dir: process.cwd(),
  }, [])

  const originalManifests = await import(path.resolve('package.json'))
  t.deepEqual(originalManifests, {
    name: 'test-publish-config',
    version: '1.0.0',

    bin: './bin.js',
    main: './index.js',
    module: './index.mjs',
    types: `./types.d.ts`,
    typings: `./typings.d.ts`,

    publishConfig: {
      bin: './published-bin.js',
      browser: './published-browser.js',
      es2015: './published-es2015.js',
      esnext: './published-esnext.js',
      exports: './published-exports.js',
      main: './published.js',
      module: './published.mjs',
      types: `./published-types.d.ts`,
      typings: `./published-typings.d.ts`,
      'umd:main': './published-umd.js',
      unpkg: './published-unpkg.js',
    },
  })

  process.chdir('../test-publish-config-installation')
  crossSpawn.sync(pnpmBin, ['add', 'test-publish-config', `--registry=http://localhost:${REGISTRY_MOCK_PORT}`])

  const publishedManifest = await import(path.resolve('node_modules/test-publish-config/package.json'))
  t.deepEqual(publishedManifest, {
    name: 'test-publish-config',
    version: '1.0.0',

    bin: './published-bin.js',
    main: './published.js',
    module: './published.mjs',
    types: './published-types.d.ts',
    typings: './published-typings.d.ts',

    publishConfig: {
      bin: './published-bin.js',
      browser: './published-browser.js',
      es2015: './published-es2015.js',
      esnext: './published-esnext.js',
      exports: './published-exports.js',
      main: './published.js',
      module: './published.mjs',
      types: './published-types.d.ts',
      typings: './published-typings.d.ts',
      'umd:main': './published-umd.js',
      unpkg: './published-unpkg.js',
    },

    browser: './published-browser.js',
    es2015: './published-es2015.js',
    esnext: './published-esnext.js',
    exports: './published-exports.js',
    'umd:main': './published-umd.js',
    unpkg: './published-unpkg.js',
  })
  t.end()
})

test.skip('publish package that calls executable from the workspace .bin folder in prepublishOnly script', async (t) => {
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

  const workspaceDir = process.cwd()
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  process.chdir('test-publish-scripts')
  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', ...CREDENTIALS] },
    dir: process.cwd(),
    workspaceDir,
  }, [])

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
    ]
  )
})

test('convert specs with workspace protocols to regular version ranges', async (t) => {
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
      devDependencies: {
        'random-package': 'workspace:^1.2.3',
      },
      optionalDependencies: {
        'lodash.deburr': 'workspace:^4.1.0',
      },
      peerDependencies: {
        'random-package': 'workspace:*',
      },
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
      name: 'random-package',
      version: '1.2.3',
    },
    {
      name: 'target',
      version: '1.0.0',
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  process.chdir('workspace-protocol-package')

  let err!: PnpmError
  try {
    await publish.handler({
      ...DEFAULT_OPTS,
      argv: { original: ['publish', ...CREDENTIALS] },
      dir: process.cwd(),
    }, [])
  } catch (_err) {
    err = _err
  }
  t.equal(err.code, 'ERR_PNPM_CANNOT_RESOLVE_WORKSPACE_PROTOCOL', 'publish fails if cannot resolve workspace:*')
  t.ok(
    err.message.includes('Cannot resolve workspace protocol of dependency "is-negative"'),
    'publish fails with the correct error message'
  )

  process.chdir('..')

  crossSpawn.sync(pnpmBin, ['multi', 'install', '--store-dir=store', `--registry=http://localhost:${REGISTRY_MOCK_PORT}`])

  process.chdir('workspace-protocol-package')
  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', ...CREDENTIALS] },
    dir: process.cwd(),
  }, [])

  process.chdir('../target')

  crossSpawn.sync(pnpmBin, ['add', '--store-dir=../store', 'workspace-protocol-package', '--no-link-workspace-packages', `--registry=http://localhost:${REGISTRY_MOCK_PORT}`])

  const publishedManifest = await import(path.resolve('node_modules/workspace-protocol-package/package.json'))
  t.deepEqual(publishedManifest.dependencies, {
    'file-type': '12.0.1',
    'is-negative': '1.0.0',
    'is-positive': '1.0.0',
    'lodash.delay': '~4.1.0',
  })
  t.deepEqual(publishedManifest.devDependencies, {
    'random-package': '^1.2.3',
  })
  t.deepEqual(publishedManifest.optionalDependencies, {
    'lodash.deburr': '^4.1.0',
  })
  t.deepEqual(publishedManifest.peerDependencies, {
    'random-package': '1.2.3',
  })
  t.end()
})

test('publish: runs all the lifecycle scripts', async (t) => {
  prepare(t, {
    name: 'test-publish-with-scripts',
    version: '0.0.0',

    dependencies: {
      'json-append': '1.1.1',
    },

    scripts: {
      // tslint:disable:object-literal-sort-keys
      prepublish: `node -e "process.stdout.write('prepublish')" | json-append output.json`,
      prepare: `node -e "process.stdout.write('prepare')" | json-append output.json`,
      prepublishOnly: `node -e "process.stdout.write('prepublishOnly')" | json-append output.json`,
      prepack: `node -e "process.stdout.write('prepack')" | json-append output.json`,
      publish: `node -e "process.stdout.write('publish')" | json-append output.json`,
      postpublish: `node -e "process.stdout.write('postpublish')" | json-append output.json`,
      // tslint:enable:object-literal-sort-keys
    },
  })

  crossSpawn.sync(pnpmBin, ['install', '--ignore-scripts', '--store-dir=../store', `--registry=http://localhost:${REGISTRY_MOCK_PORT}`])

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', ...CREDENTIALS] },
    dir: process.cwd(),
  }, [])

  const outputs = await import(path.resolve('output.json')) as string[]
  t.deepEqual(outputs, [
    'prepublish',
    'prepare',
    'prepublishOnly',
    'prepack',
    'publish',
    'postpublish',
  ])

  t.end()
})

test('publish: ignores all the lifecycle scripts when --ignore-scripts is used', async (t) => {
  prepare(t, {
    name: 'test-publish-with-ignore-scripts',
    version: '0.0.0',

    dependencies: {
      'json-append': '1.1.1',
    },

    scripts: {
      // tslint:disable:object-literal-sort-keys
      prepublish: `node -e "process.stdout.write('prepublish')" | json-append output.json`,
      prepare: `node -e "process.stdout.write('prepare')" | json-append output.json`,
      prepublishOnly: `node -e "process.stdout.write('prepublishOnly')" | json-append output.json`,
      prepack: `node -e "process.stdout.write('prepack')" | json-append output.json`,
      publish: `node -e "process.stdout.write('publish')" | json-append output.json`,
      postpublish: `node -e "process.stdout.write('postpublish')" | json-append output.json`,
      // tslint:enable:object-literal-sort-keys
    },
  })

  crossSpawn.sync(pnpmBin, ['install', '--ignore-scripts', '--store-dir=../store', `--registry=http://localhost:${REGISTRY_MOCK_PORT}`])

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', ...CREDENTIALS] },
    dir: process.cwd(),
    ignoreScripts: true,
  }, [])

  t.ok(await exists('package.json'))
  t.notOk(await exists('output.json'))

  t.end()
})

import fs from 'fs'
import path from 'path'
import execa from 'execa'
import { isCI } from 'ci-info'
import isWindows from 'is-windows'
import { getCatalogsFromWorkspaceManifest } from '@pnpm/catalogs.config'
import { pack, publish } from '@pnpm/plugin-commands-publishing'
import { prepare, preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { createTestIpcServer } from '@pnpm/test-ipc-server'
import crossSpawn from 'cross-spawn'
import { sync as writeYamlFile } from 'write-yaml-file'
import { DEFAULT_OPTS } from './utils'

const skipOnWindowsCI = isCI && isWindows() ? test.skip : test

const CREDENTIALS = [
  `--registry=http://localhost:${REGISTRY_MOCK_PORT}/`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:username=username`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:_password=${Buffer.from('password').toString('base64')}`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:email=foo@bar.net`,
]
const pnpmBin = path.join(__dirname, '../../../pnpm/bin/pnpm.cjs')

test('publish: package with package.json', async () => {
  prepare({
    name: 'test-publish-package.json',
    version: '0.0.0',
  })

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', ...CREDENTIALS] },
    dir: process.cwd(),
  }, [])

  expect(fs.existsSync('test-publish-package.json-0.0.0.tgz')).toBeFalsy()
})

test('publish: package with package.yaml', async () => {
  prepare({
    name: 'test-publish-package.yaml',
    version: '0.0.0',
  }, { manifestFormat: 'YAML' })

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', ...CREDENTIALS] },
    dir: process.cwd(),
  }, [])

  expect(fs.existsSync('package.yaml')).toBeTruthy()
  expect(fs.existsSync('package.json')).toBeFalsy()
})

test('publish: package with package.json5', async () => {
  prepare({
    name: 'test-publish-package.json5',
    version: '0.0.0',
  }, { manifestFormat: 'JSON5' })

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', ...CREDENTIALS] },
    dir: process.cwd(),
  }, [])

  expect(fs.existsSync('package.json5')).toBeTruthy()
  expect(fs.existsSync('package.json')).toBeFalsy()
})

test('publish: package with package.json5 running publish from different folder', async () => {
  prepare({
    name: 'test-publish-package.json5',
    version: '0.0.1',
  }, { manifestFormat: 'JSON5' })

  process.chdir('..')

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', ...CREDENTIALS, './project'] },
    dir: process.cwd(),
  }, ['./project'])

  expect(fs.existsSync('project/package.json5')).toBeTruthy()
  expect(fs.existsSync('project/package.json')).toBeFalsy()
})

skipOnWindowsCI('pack packages with workspace LICENSE if no own LICENSE is present', async () => {
  preparePackages([
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
  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  fs.writeFileSync('LICENSE', 'workspace license', 'utf8')
  fs.writeFileSync('project-2/LICENSE', 'project-2 license', 'utf8')

  process.chdir('project-1')
  await pack.handler({
    ...DEFAULT_OPTS,
    argv: { original: [] },
    dir: process.cwd(),
    extraBinPaths: [],
    workspaceDir,
  })

  process.chdir('../project-2')
  await pack.handler({
    ...DEFAULT_OPTS,
    argv: { original: [] },
    dir: process.cwd(),
    extraBinPaths: [],
    workspaceDir,
  })

  process.chdir('../target')

  crossSpawn.sync(pnpmBin, ['add', '../project-1/project-1-1.0.0.tgz', '../project-2/project-2-1.0.0.tgz'])

  expect(fs.existsSync('node_modules/project-1/LICENSE')).toBeTruthy()
  expect(fs.readFileSync('node_modules/project-1/LICENSE', 'utf8')).toBe('workspace license')
  expect(fs.existsSync('node_modules/project-2/LICENSE')).toBeTruthy()
  expect(fs.readFileSync('node_modules/project-2/LICENSE', 'utf8')).toBe('project-2 license')

  process.chdir('..')
  expect(fs.existsSync('project-1/LICENSE')).toBeFalsy()
  expect(fs.existsSync('project-2/LICENSE')).toBeTruthy()
})

test('publish packages with workspace LICENSE if no own LICENSE is present', async () => {
  preparePackages([
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
  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  fs.writeFileSync('LICENSE', 'workspace license', 'utf8')
  fs.writeFileSync('project-200/LICENSE', 'project-200 license', 'utf8')

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

  expect(fs.readFileSync('node_modules/project-100/LICENSE', 'utf8')).toBe('workspace license')
  expect(fs.readFileSync('node_modules/project-200/LICENSE', 'utf8')).toBe('project-200 license')

  process.chdir('..')
  expect(fs.existsSync('project-100/LICENSE')).toBeFalsy()
  expect(fs.existsSync('project-200/LICENSE')).toBeTruthy()
})

test('publish: package with all possible fields in publishConfig', async () => {
  preparePackages([
    {
      name: 'test-publish-config',
      version: '1.0.0',

      bin: './bin.js',
      main: './index.js',
      module: './index.mjs',
      types: './types.d.ts',
      typings: './typings.d.ts',

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
    },
    {
      name: 'test-publish-config-installation',
      version: '1.0.0',
    },
  ])

  process.chdir('test-publish-config')
  fs.writeFileSync('published-bin.js', '#!/usr/bin/env node', 'utf8')
  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', ...CREDENTIALS] },
    dir: process.cwd(),
  }, [])

  const { default: originalManifests } = await import(path.resolve('package.json'))
  expect(originalManifests).toStrictEqual({
    name: 'test-publish-config',
    version: '1.0.0',

    bin: './bin.js',
    main: './index.js',
    module: './index.mjs',
    types: './types.d.ts',
    typings: './typings.d.ts',

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
  })

  process.chdir('../test-publish-config-installation')
  crossSpawn.sync(pnpmBin, ['add', 'test-publish-config', `--registry=http://localhost:${REGISTRY_MOCK_PORT}`])

  const { default: publishedManifest } = await import(path.resolve('node_modules/test-publish-config/package.json'))
  expect(publishedManifest).toStrictEqual({
    name: 'test-publish-config',
    version: '1.0.0',

    bin: './published-bin.js',
    main: './published.js',
    module: './published.mjs',
    types: './published-types.d.ts',
    typings: './published-typings.d.ts',

    browser: './published-browser.js',
    es2015: './published-es2015.js',
    esnext: './published-esnext.js',
    exports: './published-exports.js',
    'umd:main': './published-umd.js',
    unpkg: './published-unpkg.js',
  })
})

test('publish: package with publishConfig.directory', async () => {
  const packages = preparePackages([
    {
      name: 'test-publish-config-directory',
      version: '1.0.0',

      scripts: {
        prepublishOnly: 'node --eval="const fs=require(\'fs\');fs.mkdirSync(\'dist\',{recursive:true});fs.writeFileSync(\'dist/prepublishOnly\', \'\', \'utf8\')"',
      },

      publishConfig: {
        directory: 'dist',
      },
    },
  ])

  const testPublishConfigDirectory = packages['test-publish-config-directory']

  expect(testPublishConfigDirectory).toBeTruthy()

  fs.mkdirSync(path.join(testPublishConfigDirectory.dir(), 'dist'))

  fs.writeFileSync(
    path.join(testPublishConfigDirectory.dir(), 'dist/package.json'),
    JSON.stringify({
      name: 'publish_config_directory_dist_package',
      version: '1.0.0',
    }),
    {
      encoding: 'utf-8',
    }
  )

  process.chdir('test-publish-config-directory')

  await publish.handler(
    {
      ...DEFAULT_OPTS,
      argv: { original: ['publish', ...CREDENTIALS] },
      dir: process.cwd(),
    },
    []
  )

  crossSpawn.sync(pnpmBin, ['add', 'publish_config_directory_dist_package', '--no-link-workspace-packages', `--registry=http://localhost:${REGISTRY_MOCK_PORT}`])

  expect(JSON.parse(fs.readFileSync('node_modules/publish_config_directory_dist_package/package.json', { encoding: 'utf-8' })))
    .toStrictEqual({
      name: 'publish_config_directory_dist_package',
      version: '1.0.0',
    })
  expect(fs.existsSync('node_modules/publish_config_directory_dist_package/prepublishOnly')).toBeTruthy()
})

test.skip('publish package that calls executable from the workspace .bin folder in prepublishOnly script', async () => {
  await using server = await createTestIpcServer()

  preparePackages([
    {
      location: '.',
      package: {
        name: 'project-100',
        version: '1.0.0',

      },
    },
    {
      name: 'test-publish-scripts',
      version: '1.0.0',

      scripts: {
        prepublish: server.sendLineScript('prepublish'),

        prepare: server.sendLineScript('prepare'),

        prepublishOnly: server.sendLineScript('prepublishOnly'),

        prepack: server.sendLineScript('prepack'),

        postpack: server.sendLineScript('postpack'),

        publish: server.sendLineScript('publish'),

        postpublish: server.sendLineScript('postpublish'),
      },
    },
  ])

  const workspaceDir = process.cwd()
  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  process.chdir('test-publish-scripts')
  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', ...CREDENTIALS] },
    dir: process.cwd(),
    workspaceDir,
  }, [])

  expect(
    server.getLines()
  ).toStrictEqual(
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

// This was broken when we started using auto-install-peers=true in the repo
test.skip('convert specs with workspace protocols to regular version ranges', async () => {
  preparePackages([
    {
      name: 'workspace-protocol-package',
      version: '1.0.0',

      dependencies: {
        even: 'workspace:is-even@^1.0.0',
        'file-type': 'workspace:12.0.1',
        'is-negative': 'workspace:*',
        'is-positive': '1.0.0',
        'lodash.delay': '~4.1.0',
        odd: 'workspace:is-odd@*',
        rd: 'workspace:ramda@^',
        'word-wrap': 'workspace:~',
      },
      devDependencies: {
        'random-package': 'workspace:^1.2.3',
        through: 'workspace:^',
      },
      optionalDependencies: {
        'lodash.deburr': 'workspace:^4.1.0',
        ww: 'workspace:wordwrap@~',
      },
      peerDependencies: {
        'random-package': 'workspace:*',
      },
    },
    {
      name: 'is-even',
      version: '1.0.0',
    },
    {
      name: 'is-odd',
      version: '1.0.0',
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
    {
      name: 'ramda',
      version: '0.1.0',
    },
    {
      name: 'word-wrap',
      version: '0.1.0',
    },
    {
      name: 'through',
      version: '0.0.1',
    },
    {
      name: 'wordwrap',
      version: '0.0.1',
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  process.chdir('workspace-protocol-package')

  await expect(
    publish.handler({
      ...DEFAULT_OPTS,
      argv: { original: ['publish', ...CREDENTIALS] },
      dir: process.cwd(),
    }, [])
  )
    .rejects
    // It would be great to match the exact error message
    // but the message will contain randomly one of the dependency names
    .toThrow(/^Cannot resolve workspace protocol of dependency "/)

  process.chdir('..')

  crossSpawn.sync(pnpmBin, ['multi', 'install', '--store-dir=store', `--registry=http://localhost:${REGISTRY_MOCK_PORT}`])

  process.chdir('workspace-protocol-package')

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', ...CREDENTIALS] },
    dir: process.cwd(),
  }, [])

  process.chdir('../target')

  crossSpawn.sync(pnpmBin, ['add', '--store-dir=store', 'workspace-protocol-package', '--no-link-workspace-packages', `--registry=http://localhost:${REGISTRY_MOCK_PORT}`])

  const { default: publishedManifest } = await import(path.resolve('node_modules/workspace-protocol-package/package.json'))
  expect(publishedManifest.dependencies).toStrictEqual({
    'file-type': '12.0.1',
    'is-negative': '1.0.0',
    'is-positive': '1.0.0',
    'lodash.delay': '~4.1.0',
    even: 'npm:is-even@^1.0.0',
    odd: 'npm:is-odd@1.0.0',
    rd: 'npm:ramda@^0.1.0',
    'word-wrap': '~0.1.0',
  })
  expect(publishedManifest.devDependencies).toStrictEqual({
    'random-package': '^1.2.3',
    through: '^0.0.1',
  })
  expect(publishedManifest.optionalDependencies).toStrictEqual({
    'lodash.deburr': '^4.1.0',
    ww: 'npm:wordwrap@~0.0.1',
  })
  expect(publishedManifest.peerDependencies).toStrictEqual({
    'random-package': '1.2.3',
  })
})

// This was broken when we started using auto-install-peers=true in the repo
test.skip('convert specs with relative workspace protocols to regular version ranges', async () => {
  preparePackages([
    {
      name: 'relative-workspace-protocol-package',
      version: '1.0.0',

      dependencies: {
        'file-type': 'workspace:../file-type',
        'is-neg': 'workspace:../is-negative',
        'is-positive': '1.0.0',
        'lodash.delay': '~4.1.0',
      },
      devDependencies: {
        'random-package': 'workspace:../random-package',
      },
      optionalDependencies: {
        'lodash.deburr': 'workspace:../lodash.deburr',
      },
      peerDependencies: {
        'random-package': 'workspace:../random-package',
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

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  process.chdir('relative-workspace-protocol-package')

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', ...CREDENTIALS] },
    dir: process.cwd(),
  }, [])

  process.chdir('../target')

  crossSpawn.sync(pnpmBin, [
    'add',
    '--store-dir=../store',
    'relative-workspace-protocol-package',
    '--no-link-workspace-packages',
    `--registry=http://localhost:${REGISTRY_MOCK_PORT}`,
  ])

  const { default: publishedManifest } = await import(path.resolve('node_modules/relative-workspace-protocol-package/package.json'))
  expect(publishedManifest.dependencies).toStrictEqual({
    'file-type': '12.0.1',
    'is-neg': 'npm:is-negative@1.0.0',
    'is-positive': '1.0.0',
    'lodash.delay': '~4.1.0',
  })
  expect(publishedManifest.devDependencies).toStrictEqual({
    'random-package': '1.2.3',
  })
  expect(publishedManifest.optionalDependencies).toStrictEqual({
    'lodash.deburr': '4.1.0',
  })
  expect(publishedManifest.peerDependencies).toStrictEqual({
    'random-package': '1.2.3',
  })
})

describe('catalog protocol converted when publishing', () => {
  test('default catalog', async () => {
    const testPackageName = 'workspace-package-with-default-catalog'
    preparePackages([
      {
        name: testPackageName,
        version: '1.0.0',
        dependencies: {
          'is-positive': 'catalog:',
        },
        devDependencies: {
          'is-positive': 'catalog:',
        },
        optionalDependencies: {
          'is-positive': 'catalog:',
        },
        peerDependencies: {
          'is-positive': 'catalog:',
        },
      },
      {
        name: 'target',
        private: true,
      },
    ])

    const workspaceManifest = {
      packages: ['**', '!store/**'],
      catalog: { 'is-positive': '1.0.0' },
    }
    writeYamlFile('pnpm-workspace.yaml', workspaceManifest)

    process.chdir(testPackageName)

    await publish.handler({
      ...DEFAULT_OPTS,
      argv: { original: ['publish', ...CREDENTIALS] },
      catalogs: getCatalogsFromWorkspaceManifest(workspaceManifest),
      dir: process.cwd(),
    }, [])

    process.chdir('../target')

    crossSpawn.sync(pnpmBin, [
      'add',
      '--store-dir=../store',
      testPackageName,
      '--no-link-workspace-packages',
      `--registry=http://localhost:${REGISTRY_MOCK_PORT}`,
    ])

    const { default: publishedManifest } = await import(path.resolve(`node_modules/${testPackageName}/package.json`))
    expect(publishedManifest.dependencies).toStrictEqual({ 'is-positive': '1.0.0' })
    expect(publishedManifest.devDependencies).toStrictEqual({ 'is-positive': '1.0.0' })
    expect(publishedManifest.optionalDependencies).toStrictEqual({ 'is-positive': '1.0.0' })
    expect(publishedManifest.peerDependencies).toStrictEqual({ 'is-positive': '1.0.0' })
  })

  test('named catalog', async () => {
    const testPackageName = 'workspace-package-with-named-catalog'
    preparePackages([
      {
        name: testPackageName,
        version: '1.0.0',
        dependencies: {
          'is-positive': 'catalog:foo',
        },
        devDependencies: {
          'is-positive': 'catalog:bar',
        },
        optionalDependencies: {
          'is-positive': 'catalog:baz',
        },
        peerDependencies: {
          'is-positive': 'catalog:qux',
        },
      },
      {
        name: 'target',
        private: true,
      },
    ])

    const workspaceManifest = {
      packages: ['**', '!store/**'],
      catalogs: {
        foo: { 'is-positive': '1.0.0' },
        bar: { 'is-positive': '1.0.0' },
        baz: { 'is-positive': '1.0.0' },
        qux: { 'is-positive': '1.0.0' },
      },
    }
    writeYamlFile('pnpm-workspace.yaml', workspaceManifest)

    process.chdir(testPackageName)

    await publish.handler({
      ...DEFAULT_OPTS,
      argv: { original: ['publish', ...CREDENTIALS] },
      catalogs: getCatalogsFromWorkspaceManifest(workspaceManifest),
      dir: process.cwd(),
    }, [])

    process.chdir('../target')

    crossSpawn.sync(pnpmBin, [
      'add',
      '--store-dir=../store',
      testPackageName,
      '--no-link-workspace-packages',
      `--registry=http://localhost:${REGISTRY_MOCK_PORT}`,
    ])

    const { default: publishedManifest } = await import(path.resolve(`node_modules/${testPackageName}/package.json`))
    expect(publishedManifest.dependencies).toStrictEqual({ 'is-positive': '1.0.0' })
    expect(publishedManifest.devDependencies).toStrictEqual({ 'is-positive': '1.0.0' })
    expect(publishedManifest.optionalDependencies).toStrictEqual({ 'is-positive': '1.0.0' })
    expect(publishedManifest.peerDependencies).toStrictEqual({ 'is-positive': '1.0.0' })
  })
})

test('publish: runs all the lifecycle scripts', async () => {
  await using server = await createTestIpcServer()

  prepare({
    name: 'test-publish-with-scripts',
    version: '0.0.0',

    scripts: {
      // eslint-disable:object-literal-sort-keys
      prepublish: server.sendLineScript('prepublish'),
      prepare: server.sendLineScript('prepare'),
      prepublishOnly: server.sendLineScript('prepublishOnly'),
      prepack: server.sendLineScript('prepack'),
      publish: server.sendLineScript('publish'),
      postpublish: server.sendLineScript('postpublish'),
      // eslint-enable:object-literal-sort-keys
    },
  })

  crossSpawn.sync(pnpmBin, ['install', '--ignore-scripts', '--store-dir=../store', `--registry=http://localhost:${REGISTRY_MOCK_PORT}`])

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', ...CREDENTIALS] },
    dir: process.cwd(),
  }, [])

  expect(server.getLines()).toStrictEqual([
    'prepublishOnly',
    'prepublish',
    'prepack',
    'prepare',
    'publish',
    'postpublish',
  ])
})

test('publish: ignores all the lifecycle scripts when --ignore-scripts is used', async () => {
  await using server = await createTestIpcServer()

  prepare({
    name: 'test-publish-with-ignore-scripts',
    version: '0.0.0',

    scripts: {
      // eslint-disable:object-literal-sort-keys
      prepublish: server.sendLineScript('prepublish'),
      prepare: server.sendLineScript('prepare'),
      prepublishOnly: server.sendLineScript('prepublishOnly'),
      prepack: server.sendLineScript('prepack'),
      publish: server.sendLineScript('publish'),
      postpublish: server.sendLineScript('postpublish'),
      // eslint-enable:object-literal-sort-keys
    },
  })

  crossSpawn.sync(pnpmBin, ['install', '--ignore-scripts', '--store-dir=../store', `--registry=http://localhost:${REGISTRY_MOCK_PORT}`])

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', ...CREDENTIALS] },
    dir: process.cwd(),
    ignoreScripts: true,
  }, [])

  expect(fs.existsSync('package.json')).toBeTruthy()
  expect(server.getLines()).toStrictEqual([])
})

test('publish: with specified publish branch name', async () => {
  prepare({
    name: 'test-publish-package.json',
    version: '0.0.2',
  })

  const branch = 'some-random-publish-branch'
  await execa('git', ['init', `--initial-branch=${branch}`])
  await execa('git', ['config', 'user.email', 'x@y.z'])
  await execa('git', ['config', 'user.name', 'xyz'])
  await execa('git', ['add', '*'])
  await execa('git', ['commit', '-m', 'init', '--no-gpg-sign'])

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', '--publish-branch', branch, ...CREDENTIALS] },
    dir: process.cwd(),
    publishBranch: branch,
  }, [])
})

test('publish: exit with non-zero code when publish tgz', async () => {
  prepare({
    name: 'test-publish-package.json',
    version: '0.0.2',
  })

  const result = await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', './non-exists.tgz', '--no-git-checks'] },
    dir: process.cwd(),
    gitChecks: false,

  }, [
    './non-exists.tgz',
  ])
  expect(result?.exitCode).not.toBe(0)
})

test('publish: provenance', async () => {
  prepare({
    name: 'test-publish-package.json',
    version: '0.0.2',
  })

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', '--provenance'] },
    dir: process.cwd(),
  }, [])
})

test('publish: use basic token helper for authentication', async () => {
  prepare({
    name: 'test-publish-helper-token-basic.json',
    version: '0.0.2',
  })

  const os = process.platform
  const file = os === 'win32'
    ? 'tokenHelperBasic.bat'
    : 'tokenHelperBasic.js'

  const tokenHelper = path.join(__dirname, 'utils', file)

  fs.chmodSync(tokenHelper, 0o755)

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: {
      original: [
        'publish',
        CREDENTIALS[0],
        `--//localhost:${REGISTRY_MOCK_PORT}/:tokenHelper=${tokenHelper}`,
      ],
    },
    dir: process.cwd(),
    gitChecks: false,
  }, [])
})

test('publish: use bearer token helper for authentication', async () => {
  prepare({
    name: 'test-publish-helper-token-bearer.json',
    version: '0.0.2',
  })

  const os = process.platform
  const file = os === 'win32'
    ? 'tokenHelperBearer.bat'
    : 'tokenHelperBearer.js'
  const tokenHelper = path.join(__dirname, 'utils', file)

  fs.chmodSync(tokenHelper, 0o755)

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: {
      original: [
        'publish',
        CREDENTIALS[0],
        `--//localhost:${REGISTRY_MOCK_PORT}/:tokenHelper=${tokenHelper}`,
      ],
    },
    dir: process.cwd(),
    gitChecks: false,
  }, [])
})

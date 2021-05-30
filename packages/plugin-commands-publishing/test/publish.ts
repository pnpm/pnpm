import { promises as fs } from 'fs'
import path from 'path'
import execa from 'execa'
import isCI from 'is-ci'
import isWindows from 'is-windows'
import { pack, publish } from '@pnpm/plugin-commands-publishing'
import prepare, { preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import exists from 'path-exists'
import crossSpawn from 'cross-spawn'
import writeYamlFile from 'write-yaml-file'
import { DEFAULT_OPTS } from './utils'

const skipOnWindowsCI = isCI && isWindows() ? test.skip : test

const CREDENTIALS = [
  `--registry=http://localhost:${REGISTRY_MOCK_PORT}/`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:username=username`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:_password=${Buffer.from('password').toString('base64')}`,
  `--//localhost:${REGISTRY_MOCK_PORT}/:email=foo@bar.net`,
]
const pnpmBin = path.join(__dirname, '../../pnpm/bin/pnpm.cjs')

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

  expect(await exists('package.yaml')).toBeTruthy()
  expect(await exists('package.json')).toBeFalsy()
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

  expect(await exists('package.json5')).toBeTruthy()
  expect(await exists('package.json')).toBeFalsy()
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

  expect(await exists('project/package.json5')).toBeTruthy()
  expect(await exists('project/package.json')).toBeFalsy()
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
  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await fs.writeFile('LICENSE', 'workspace license', 'utf8')
  await fs.writeFile('project-2/LICENSE', 'project-2 license', 'utf8')

  process.chdir('project-1')
  await pack.handler({ argv: { original: [] }, dir: process.cwd(), workspaceDir })

  process.chdir('../project-2')
  await pack.handler({ argv: { original: [] }, dir: process.cwd(), workspaceDir })

  process.chdir('../target')

  crossSpawn.sync(pnpmBin, ['add', '../project-1/project-1-1.0.0.tgz', '../project-2/project-2-1.0.0.tgz'])

  expect(await exists('node_modules/project-1/LICENSE')).toBeTruthy()
  expect(await fs.readFile('node_modules/project-1/LICENSE', 'utf8')).toBe('workspace license')
  expect(await exists('node_modules/project-2/LICENSE')).toBeTruthy()
  expect(await fs.readFile('node_modules/project-2/LICENSE', 'utf8')).toBe('project-2 license')

  process.chdir('..')
  expect(await exists('project-1/LICENSE')).toBeFalsy()
  expect(await exists('project-2/LICENSE')).toBeTruthy()
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

  expect(await fs.readFile('node_modules/project-100/LICENSE', 'utf8')).toBe('workspace license')
  expect(await fs.readFile('node_modules/project-200/LICENSE', 'utf8')).toBe('project-200 license')

  process.chdir('..')
  expect(await exists('project-100/LICENSE')).toBeFalsy()
  expect(await exists('project-200/LICENSE')).toBeTruthy()
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
  await fs.writeFile('published-bin.js', '#!/usr/bin/env node', 'utf8')
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
})

test('publish: package with publishConfig.directory', async () => {
  const packages = preparePackages([
    {
      name: 'test-publish-config-directory',
      version: '1.0.0',

      publishConfig: {
        directory: 'dist',
      },
    },
  ])

  const testPublishConfigDirectory = packages['test-publish-config-directory']

  expect(testPublishConfigDirectory).toBeTruthy()

  await fs.mkdir(path.join(testPublishConfigDirectory.dir(), 'dist'))

  await fs.writeFile(
    path.join(testPublishConfigDirectory.dir(), 'dist/package.json'),
    `
    {
      "name": "publish_config_directory_dist_package",
      "version": "1.0.0"
    }
  `,
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

  expect(JSON.parse(await fs.readFile('node_modules/publish_config_directory_dist_package/package.json', { encoding: 'utf-8' })))
    .toStrictEqual({
      name: 'publish_config_directory_dist_package',
      version: '1.0.0',
    })
})

test.skip('publish package that calls executable from the workspace .bin folder in prepublishOnly script', async () => {
  preparePackages([
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
        prepublish: 'node -e "process.stdout.write(\'prepublish\')" | json-append ./output.json',

        prepare: 'node -e "process.stdout.write(\'prepare\')" | json-append ./output.json',

        prepublishOnly: 'node -e "process.stdout.write(\'prepublishOnly\')" | json-append ./output.json',

        prepack: 'node -e "process.stdout.write(\'prepack\')" | json-append ./output.json',

        postpack: 'node -e "process.stdout.write(\'postpack\')" | json-append ./output.json',

        publish: 'node -e "process.stdout.write(\'publish\')" | json-append ./output.json',

        postpublish: 'node -e "process.stdout.write(\'postpublish\')" | json-append ./output.json',
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

  expect(
    (await import(path.resolve('output.json'))).default
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

test('convert specs with workspace protocols to regular version ranges', async () => {
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

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

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

  crossSpawn.sync(pnpmBin, ['add', '--store-dir=../store', 'workspace-protocol-package', '--no-link-workspace-packages', `--registry=http://localhost:${REGISTRY_MOCK_PORT}`])

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

test('convert specs with relative workspace protocols to regular version ranges', async () => {
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

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

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

test('publish: runs all the lifecycle scripts', async () => {
  prepare({
    name: 'test-publish-with-scripts',
    version: '0.0.0',

    dependencies: {
      'json-append': '1.1.1',
    },

    scripts: {
      // eslint-disable:object-literal-sort-keys
      prepublish: 'node -e "process.stdout.write(\'prepublish\')" | json-append output.json',
      prepare: 'node -e "process.stdout.write(\'prepare\')" | json-append output.json',
      prepublishOnly: 'node -e "process.stdout.write(\'prepublishOnly\')" | json-append output.json',
      prepack: 'node -e "process.stdout.write(\'prepack\')" | json-append output.json',
      publish: 'node -e "process.stdout.write(\'publish\')" | json-append output.json',
      postpublish: 'node -e "process.stdout.write(\'postpublish\')" | json-append output.json',
      // eslint-enable:object-literal-sort-keys
    },
  })

  crossSpawn.sync(pnpmBin, ['install', '--ignore-scripts', '--store-dir=../store', `--registry=http://localhost:${REGISTRY_MOCK_PORT}`])

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish', ...CREDENTIALS] },
    dir: process.cwd(),
  }, [])

  const { default: outputs } = await import(path.resolve('output.json'))
  expect(outputs).toStrictEqual([
    'prepublish',
    'prepare',
    'prepublishOnly',
    'prepack',
    'publish',
    'postpublish',
  ])
})

test('publish: ignores all the lifecycle scripts when --ignore-scripts is used', async () => {
  prepare({
    name: 'test-publish-with-ignore-scripts',
    version: '0.0.0',

    dependencies: {
      'json-append': '1.1.1',
    },

    scripts: {
      // eslint-disable:object-literal-sort-keys
      prepublish: 'node -e "process.stdout.write(\'prepublish\')" | json-append output.json',
      prepare: 'node -e "process.stdout.write(\'prepare\')" | json-append output.json',
      prepublishOnly: 'node -e "process.stdout.write(\'prepublishOnly\')" | json-append output.json',
      prepack: 'node -e "process.stdout.write(\'prepack\')" | json-append output.json',
      publish: 'node -e "process.stdout.write(\'publish\')" | json-append output.json',
      postpublish: 'node -e "process.stdout.write(\'postpublish\')" | json-append output.json',
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

  expect(await exists('package.json')).toBeTruthy()
  expect(await exists('output.json')).toBeFalsy()
})

test('publish: with specified publish branch name', async () => {
  prepare({
    name: 'test-publish-package.json',
    version: '0.0.2',
  })

  const branch = 'some-random-publish-branch'
  await execa('git', ['init'])
  await execa('git', ['checkout', '-b', branch])
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

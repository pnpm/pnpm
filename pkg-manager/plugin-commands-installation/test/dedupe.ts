import fs from 'fs'
import path from 'path'
import { DedupeCheckIssuesError } from '@pnpm/dedupe.check'
import { readProjects } from '@pnpm/filter-workspace-packages'
import { type Lockfile } from '@pnpm/lockfile-types'
import { dedupe, install } from '@pnpm/plugin-commands-installation'
import { prepare } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import rimraf from '@zkochan/rimraf'
import execa from 'execa'
import { diff } from 'jest-diff'
import readYamlFile from 'read-yaml-file'
import { DEFAULT_OPTS, REGISTRY } from './utils'

const f = fixtures(__dirname)
const pnpmBin = path.join(__dirname, '../../../pnpm/bin/pnpm.cjs')

describe('pnpm dedupe', () => {
  test('updates old resolutions from importers block and removes old packages', async () => {
    const { originalLockfile, dedupedLockfile, dedupeCheckError } = await testFixture('workspace-with-lockfile-dupes')
    // Many old packages should be deleted as result of deduping. See snapshot file for details.
    expect(diff(originalLockfile, dedupedLockfile, diffOptsForLockfile)).toMatchSnapshot()
    expect(dedupeCheckError.dedupeCheckIssues).toEqual({
      importerIssuesByImporterId: {
        added: [],
        removed: [],
        updated: {
          'packages/bar': {
            ajv: {
              next: '6.12.6',
              prev: '6.10.2',
              type: 'updated',
            },
          },
        },
      },
      packageIssuesByDepPath: {
        added: [],
        removed: [
          '/ajv/6.10.2',
          '/fast-deep-equal/2.0.1',
          '/fast-json-stable-stringify/2.0.0',
          '/punycode/2.1.1',
          '/uri-js/4.2.2',
        ],
        updated: {},
      },
    })
  })

  test('updates old resolutions from package block', async () => {
    const { originalLockfile, dedupedLockfile, dedupeCheckError } = await testFixture('workspace-with-lockfile-subdep-dupes')
    // This is a smaller scale test that should just update uri-js@4.2.2 to
    // punycode@2.3.0 and remove punycode@2.1.1. See snapshot file for details.
    expect(diff(originalLockfile, dedupedLockfile, diffOptsForLockfile)).toMatchSnapshot()
    expect(dedupeCheckError.dedupeCheckIssues).toEqual({
      importerIssuesByImporterId: {
        added: [],
        removed: [],
        updated: {},
      },
      packageIssuesByDepPath: {
        added: [],
        removed: [
          '/punycode/2.1.1',
        ],
        updated: {
          '/uri-js/4.2.2': {
            punycode: {
              next: '2.3.0',
              prev: '2.1.1',
              type: 'updated',
            },
          },
        },
      },
    })
  })

  test('dedupe: ignores all the lifecycle scripts when --ignore-scripts is used', async () => {
    const project = prepare({
      name: 'test-dedupe-with-ignore-scripts',
      version: '0.0.0',

      dependencies: {
        'json-append': '1.1.1',
      },

      scripts: {
        // eslint-disable:object-literal-sort-keys
        preinstall: 'node -e "process.stdout.write(\'preinstall\')" | json-append output.json',
        prepare: 'node -e "process.stdout.write(\'prepare\')" | json-append output.json',
        postinstall: 'node -e "process.stdout.write(\'postinstall\')" | json-append output.json',
        // eslint-enable:object-literal-sort-keys
      },
    })

    const opts = {
      ...DEFAULT_OPTS,
      recursive: true,
      dir: project.dir(),
      ignoreScripts: true,
      lockfileDir: project.dir(),
      workspaceDir: project.dir(),
    }

    await install.handler(opts)

    await dedupe.handler(opts)

    expect(fs.existsSync('package.json')).toBeTruthy()
    expect(fs.existsSync('output.json')).toBeFalsy()
  })

  describe('respects .npmrc options', () => {
    test('uses store-dir .npmrc option', async () => {
      const project = prepare({
        dependencies: {
          'is-positive': '3.1.0',
        },
      })

      fs.writeFileSync(path.join(project.dir(), '.npmrc'), 'store-dir = local-store-dir')
      const storeDir = path.join(project.dir(), 'local-store-dir')

      // Sanity check that the first "pnpm install" creates the store in the expected location.
      await execa('node', [
        pnpmBin,
        'install',
        `--registry=${REGISTRY}`,
        `--cache-dir=${path.resolve(DEFAULT_OPTS.cacheDir)}`,
      ])
      expect(fs.existsSync(storeDir)).toBeTruthy()

      // Running "pnpm dedupe" should recreate the store dir. Clear it to ensure this happens.
      await rimraf(storeDir)
      await rimraf(path.join(project.dir(), 'node_modules'))
      expect(fs.existsSync(storeDir)).toBeFalsy()

      await execa('node', [
        pnpmBin,
        'dedupe',
        `--registry=${REGISTRY}`,
        `--cache-dir=${path.resolve(DEFAULT_OPTS.cacheDir)}`,
      ])
      expect(fs.existsSync(storeDir)).toBeTruthy()
    })
  })

  describe('respects cli args from install command', () => {
    test('uses --store-dir arg', async () => {
      const project = prepare({
        dependencies: {
          'is-positive': '3.1.0',
        },
      })

      // Sanity check that the first "pnpm install" creates the store in the expected location.
      await execa('node', [
        pnpmBin,
        'install',
        `--registry=${REGISTRY}`,
        `--cache-dir=${path.resolve(DEFAULT_OPTS.cacheDir)}`,
        '--store-dir=local-store-dir',
      ])
      const storeDir = path.join(project.dir(), 'local-store-dir')
      expect(fs.existsSync(storeDir)).toBeTruthy()

      // Running "pnpm dedupe" should recreate the store dir. Clear it to ensure this happens.
      await rimraf(storeDir)
      await rimraf(path.join(project.dir(), 'node_modules'))
      expect(fs.existsSync(storeDir)).toBeFalsy()

      await execa('node', [
        pnpmBin,
        'dedupe',
        `--registry=${REGISTRY}`,
        `--cache-dir=${path.resolve(DEFAULT_OPTS.cacheDir)}`,
        '--store-dir=local-store-dir',
      ])
      expect(fs.existsSync(storeDir)).toBeTruthy()
    })
  })
})

const noColor = (str: string) => str
const diffOptsForLockfile = {
  // Avoid showing common lines to make the snapshot smaller and less noisy.
  // https://github.com/facebook/jest/tree/05deb8393c4ad71/packages/jest-diff#example-of-options-to-limit-common-lines
  contextLines: 3,
  expand: false,

  // Remove color from snapshots
  // https://github.com/facebook/jest/tree/05deb8393c4ad71/packages/jest-diff#example-of-options-for-no-colors
  aColor: noColor,
  bColor: noColor,
  changeColor: noColor,
  commonColor: noColor,
  patchColor: noColor,
}

async function testFixture (fixtureName: string) {
  const project = prepare(undefined)
  f.copy(fixtureName, project.dir())

  const { allProjects, selectedProjectsGraph } = await readProjects(project.dir(), [])

  const opts = {
    ...DEFAULT_OPTS,
    allProjects,
    selectedProjectsGraph,
    recursive: true,
    dir: project.dir(),
    lockfileDir: project.dir(),
    workspaceDir: project.dir(),
    resolutionMode: 'highest' as const, // TODO: this should work with the default resolution mode (TODOv8)
  }

  const readProjectLockfile = () => readYamlFile<Lockfile>(path.join(project.dir(), './pnpm-lock.yaml'))

  const originalLockfile = await readProjectLockfile()

  // Sanity check that this test is set up correctly by ensuring the lockfile is
  // unmodified after a regular install.
  await install.handler(opts)
  expect(await readProjectLockfile()).toEqual(originalLockfile)

  let dedupeCheckError: DedupeCheckIssuesError | undefined
  try {
    await dedupe.handler({ ...opts, check: true })
  } catch (err: unknown) {
    expect(err).toBeInstanceOf(DedupeCheckIssuesError)
    dedupeCheckError = err as DedupeCheckIssuesError
  } finally {
    // The dedupe check option should never change the lockfile.
    expect(await readProjectLockfile()).toEqual(originalLockfile)
  }

  if (dedupeCheckError == null) {
    throw new Error('Expected change report from pnpm dedupe --check')
  }

  // The lockfile fixture has several packages that could be removed after
  // re-resolving versions.
  await dedupe.handler(opts)

  const dedupedLockfile = await readProjectLockfile()

  // It should be possible to remove packages from the fixture lockfile.
  const originalLockfilePackageNames = Object.keys(originalLockfile.packages ?? {})
  const dedupedLockfilePackageNames = Object.keys(dedupedLockfile.packages ?? {})
  expect(dedupedLockfilePackageNames.length).toBeLessThan(originalLockfilePackageNames.length)

  // The "pnpm dedupe" command should only remove packages when the lockfile is
  // up to date. Ensure no new packages/dependencies were added.
  expect(originalLockfilePackageNames).toEqual(expect.arrayContaining(dedupedLockfilePackageNames))

  // Run pnpm install one last time to ensure the deduped lockfile is in a good
  // state. If so, the "pnpm install" command should pass successfully and not
  // make any further edits to the lockfile.
  await install.handler(opts)
  expect(await readProjectLockfile()).toEqual(dedupedLockfile)

  return { originalLockfile, dedupedLockfile, dedupeCheckError }
}

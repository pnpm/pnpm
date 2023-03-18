import path from 'path'
import { readProjects } from '@pnpm/filter-workspace-packages'
import { type Lockfile } from '@pnpm/lockfile-types'
import { dedupe, install } from '@pnpm/plugin-commands-installation'
import { prepare } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { diff } from 'jest-diff'
import readYamlFile from 'read-yaml-file'
import { DEFAULT_OPTS } from './utils'

const f = fixtures(__dirname)

describe('pnpm dedupe', () => {
  test('updates old resolutions from importers block and removes old packages', async () => {
    const { originalLockfile, dedupedLockfile } = await testFixture('workspace-with-lockfile-dupes')
    // Many old packages should be deleted as result of deduping. See snapshot file for details.
    expect(diff(originalLockfile, dedupedLockfile, diffOptsForLockfile)).toMatchSnapshot()
  })

  test('updates old resolutions from package block', async () => {
    const { originalLockfile, dedupedLockfile } = await testFixture('workspace-with-lockfile-subdep-dupes')
    // This is a smaller scale test that should just update uri-js@4.2.2 to
    // punycode@2.3.0 and remove punycode@2.1.1. See snapshot file for details.
    expect(diff(originalLockfile, dedupedLockfile, diffOptsForLockfile)).toMatchSnapshot()
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

  return { originalLockfile, dedupedLockfile }
}

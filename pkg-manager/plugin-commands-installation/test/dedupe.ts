import path from 'path'
import { readProjects } from '@pnpm/filter-workspace-packages'
import { dedupe, install } from '@pnpm/plugin-commands-installation'
import { prepare } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import readYamlFile from 'read-yaml-file'
import { DEFAULT_OPTS } from './utils'
import { Lockfile } from '@pnpm/lockfile-types'

const f = fixtures(__dirname)

test('removes packages from pnpm-lock.yaml', async () => {
  const project = prepare(undefined)
  f.copy('workspace-with-lockfile-dupes', project.dir())

  const { allProjects, selectedProjectsGraph } = await readProjects(project.dir(), [])

  const opts = {
    ...DEFAULT_OPTS,
    allProjects,
    selectedProjectsGraph,
    recursive: true,
    dir: project.dir(),
    lockfileDir: project.dir(),
    workspaceDir: project.dir(),
  }

  const readProjectLockfile = () => readYamlFile<Lockfile>(path.join(project.dir(), './pnpm-lock.yaml'))

  const originalLockfile = await readProjectLockfile()

  // Sanity check that this test is set up correctly by ensuring the lockfile is
  // unmodified after a regular install.
  expect(originalLockfile).toMatchSnapshot()
  await install.handler(opts)
  expect(await readProjectLockfile()).toEqual(originalLockfile)

  // The lockfile fixture has several packages that could be removed after
  // re-resolving versions.
  await dedupe.handler(opts)

  const dedupedLockfile = await readProjectLockfile()
  expect(dedupedLockfile).toMatchSnapshot()

  // It's possible to remove many packages from the fixture lockfile.
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
})

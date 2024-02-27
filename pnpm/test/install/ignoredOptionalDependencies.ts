import { type ProjectManifest } from '@pnpm/types'
import { prepare } from '@pnpm/prepare'
import { writeProjectManifest } from '@pnpm/write-project-manifest'
import { execPnpm } from '../utils'

test('adding or changing manifest.pnpm.ignoredOptionalDependencies should change lockfile.ignoredOptionalDependencies and module structure', async () => {
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-good-optional': '1.0.0',
    },
  }
  const project = prepare(manifest)
  await execPnpm(['install'])
  {
    const lockfile = project.readLockfile()
    expect(lockfile).not.toHaveProperty(['ignoredOptionalDependencies'])
    expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/pkg-with-good-optional@1.0.0'])
    expect(lockfile.packages).toHaveProperty(['/is-positive@1.0.0'])
  }

  await writeProjectManifest('package.json', {
    ...manifest,
    pnpm: {
      ...manifest.pnpm,
      ignoredOptionalDependencies: ['is-positive'],
    },
  })
  await execPnpm(['install'])
  {
    const lockfile = project.readLockfile()
    expect(lockfile.ignoredOptionalDependencies).toStrictEqual(['is-positive'])
    expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/pkg-with-good-optional@1.0.0'])
    expect(lockfile.packages).not.toHaveProperty(['/is-positive@1.0.0'])
  }
})

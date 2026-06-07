import type { ProjectManifest } from '@pnpm/types'

import type { ExportedManifest } from './index.js'

type Input = Pick<ProjectManifest, 'repository'>
type Output<Manifest extends Input> = Omit<Manifest, 'repository'> & Pick<ExportedManifest, 'repository'>

/**
 * Normalizes a string `repository` into the object form `{ type: 'git', url }`.
 *
 * npm's `normalize-package-data` performs this conversion before publishing, so a
 * package whose `repository` is a bare URL string still reaches the registry as an
 * object. Some registries (e.g. Gitea) reject a string `repository` with a 500
 * because they decode it into an object-typed struct. Matching npm keeps
 * `pnpm publish` compatible with them. See https://github.com/pnpm/pnpm/issues/12099.
 */
export function transformRepository<Manifest extends Input> (manifest: Manifest): Output<Manifest> {
  if (typeof manifest.repository !== 'string') return manifest as Output<Manifest>
  return {
    ...manifest,
    repository: {
      type: 'git',
      url: manifest.repository,
    },
  }
}

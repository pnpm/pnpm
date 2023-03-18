import { type ProjectSnapshot } from '@pnpm/lockfile-types'
import { type DependenciesField } from '@pnpm/types'

export function filterImporter (
  importer: ProjectSnapshot,
  include: { [dependenciesField in DependenciesField]: boolean }
): ProjectSnapshot {
  return {
    dependencies: !include.dependencies ? {} : importer.dependencies ?? {},
    devDependencies: !include.devDependencies ? {} : importer.devDependencies ?? {},
    optionalDependencies: !include.optionalDependencies ? {} : importer.optionalDependencies ?? {},
    specifiers: importer.specifiers,
  }
}

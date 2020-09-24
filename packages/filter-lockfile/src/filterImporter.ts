import { ProjectSnapshot } from '@pnpm/lockfile-types'
import { DependenciesField } from '@pnpm/types'

export default function filterImporter (
  importer: ProjectSnapshot,
  include: { [dependenciesField in DependenciesField]: boolean }
) {
  return {
    dependencies: !include.dependencies ? {} : importer.dependencies ?? {},
    devDependencies: !include.devDependencies ? {} : importer.devDependencies ?? {},
    optionalDependencies: !include.optionalDependencies ? {} : importer.optionalDependencies ?? {},
    specifiers: importer.specifiers,
  }
}

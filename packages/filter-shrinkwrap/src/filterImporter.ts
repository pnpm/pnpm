import { ShrinkwrapImporter } from '@pnpm/shrinkwrap-types'
import { DependenciesField } from '@pnpm/types'

export default function filterImporter (
  importer: ShrinkwrapImporter,
  include: { [dependenciesField in DependenciesField]: boolean },
) {
  return {
    dependencies: !include.dependencies ? {} : importer.dependencies || {},
    devDependencies: !include.devDependencies ? {} : importer.devDependencies || {},
    optionalDependencies: !include.optionalDependencies ? {} : importer.optionalDependencies || {},
    specifiers: importer.specifiers,
  }
}

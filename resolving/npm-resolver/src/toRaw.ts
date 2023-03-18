import { type RegistryPackageSpec } from './parsePref'

export function toRaw (spec: RegistryPackageSpec) {
  return `${spec.name}@${spec.fetchSpec}`
}

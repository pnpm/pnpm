import { RegistryPackageSpec } from './parsePref'

export function toRaw (spec: RegistryPackageSpec) {
  return `${spec.name}@${spec.fetchSpec}`
}

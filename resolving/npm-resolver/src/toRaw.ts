import { type RegistryPackageSpec } from './parsePref'

export function toRaw (spec: RegistryPackageSpec): string {
  return `${spec.name}@${spec.fetchSpec}`
}

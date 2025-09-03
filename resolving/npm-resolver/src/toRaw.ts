import { type RegistryPackageSpec } from './parseBareSpecifier.js'

export function toRaw (spec: RegistryPackageSpec): string {
  return `${spec.name}@${spec.fetchSpec}`
}

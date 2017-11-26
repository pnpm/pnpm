import {RegistryPackageSpec} from './parsePref'

export default function toRaw (spec: RegistryPackageSpec) {
  return `${spec.name}@${spec.fetchSpec}`
}

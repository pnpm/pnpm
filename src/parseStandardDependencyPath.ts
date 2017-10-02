import {isAbsolute} from 'dependency-path'

export default function (dependencyPath: string) {
  let parts = dependencyPath.split('/')
  if (isAbsolute(dependencyPath)) {
    parts = parts.slice(2)
  }
  if (parts[1][0] === '@') {
    return {
      name: `${parts[1]}/${parts[2]}`,
      version: parts[3],
    }
  }
  return {
    name: parts[1],
    version: parts[2],
  }
}

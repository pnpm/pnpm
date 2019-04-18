import {
  DependencyType,
  Logger,
  NullableDependencies,
  PackageJson,
  ReadPackageUtils,
} from './types'

const VALID_DEPENDENCY_TYPES = ([
  'dependencies',
  'devDependencies',
  'optionalDependencies',
  'peerDependencies',
] as unknown) as TypeGuardReadonlyArray<DependencyType>
/** @see https://github.com/Microsoft/TypeScript/issues/31018 */
interface TypeGuardReadonlyArray<T extends string> extends ReadonlyArray<T> {
  includes (searchElement: string, fromIndex?: number): searchElement is T
}

/** A custom `typeof` function which handles `null` correctly. */
function _typeof (obj: unknown) {
  return obj === null ? 'null' : typeof obj
}

interface WrappedLogger {
  log (message?: unknown, ...optionalParams: unknown[]): void
}

export function createReadPackageUtils (
  pkg: PackageJson,
  logger: Logger,
): ReadPackageUtils {
  const messages: string[] = []
  const wrappedLogger: WrappedLogger = {
    log () {
      messages.push(
        arguments.length > 0
          ? `   - ${Array.prototype.join.call(arguments, ' ')}`
          : '',
      )
    },
  }

  const _setDep = setDep.bind(null, pkg, wrappedLogger)
  const _setDeps = (
    dependencyMap: NullableDependencies,
    type: DependencyType,
  ) => {
    for (const dependency of Object.keys(dependencyMap)) {
      let target = dependencyMap[dependency]
      if (typeof target === 'undefined') {
        continue
      }
      if (typeof target !== 'string' && target !== null) {
        throw new TypeError(
          `Type of ${dependency}'s value must be a string or null, got ${typeof target}`,
        )
      }
      _setDep(dependency, target, type)
    }
  }

  return {
    setDependency (dependency, target, type) {
      if (typeof target !== 'string') {
        throw new TypeError('Target must be a string')
      }
      if (type !== undefined && !VALID_DEPENDENCY_TYPES.includes(type)) {
        throw new TypeError(
          `Type must be one of ${VALID_DEPENDENCY_TYPES}, got "${type}"`,
        )
      }
      _setDep(dependency, target, type)
    },
    removeDependency (dependency, type) {
      if (type !== undefined && !VALID_DEPENDENCY_TYPES.includes(type)) {
        throw new TypeError(
          `Type must be one of ${VALID_DEPENDENCY_TYPES}, got "${type}"`,
        )
      }
      _setDep(dependency, null, type)
    },
    setDependencies (dependencyMap: unknown, type?: unknown) {
      if (typeof type === 'string') {
        if (!VALID_DEPENDENCY_TYPES.includes(type)) {
          throw new TypeError(
            `Type must be one of ${VALID_DEPENDENCY_TYPES} or undefined, got "${type}"`,
          )
        }
        if (typeof dependencyMap !== 'object' || dependencyMap === null) {
          throw new TypeError(
            `DependencyMap must be an object, got ${_typeof(dependencyMap)}`,
          )
        }
        return _setDeps(dependencyMap as NullableDependencies, type)
      } else if (typeof type !== 'undefined') {
        throw new TypeError(
          `Type must be one of ${VALID_DEPENDENCY_TYPES} or undefined, got "${type}"`,
        )
      }
      if (typeof dependencyMap !== 'object' || dependencyMap === null) {
        throw new TypeError(
          `DependencyMap must be an object, got ${_typeof(dependencyMap)}`,
        )
      }
      const types = Object.keys(dependencyMap)
      for (const type of types) {
        if (!VALID_DEPENDENCY_TYPES.includes(type)) {
          continue
        }
        const dependencies = dependencyMap[type] as NullableDependencies
        if (typeof dependencies === 'undefined') {
          continue
        }
        if (typeof dependencies !== 'object' || dependencies === null) {
          throw new TypeError(
            `DependencyMap's ${type} property must be an object, got ${_typeof(dependencies)}`,
          )
        }
        _setDeps(dependencies, type)
      }
    },
    log: wrappedLogger.log.bind(wrappedLogger),
    logChanges () {
      if (messages.length > 0) {
        logger.log(`\n  Editing "${pkg.name}@${pkg.version}":\n${
          messages.join('\n')
        }`)
        messages.splice(0)
        return true
      }
      return false
    },
  }
}

/**
 * @param pkg The `package.json` file's contents
 * @param logger The `Logger`
 * @param dependency The dependency name
 * @param target The target version, or `null` to remove
 * @param type The dependency type.
 */
function setDep (
  pkg: PackageJson,
  logger: WrappedLogger,
  dependency: string,
  target: string | null = null,
  type: DependencyType = 'dependencies',
) {
  const dependencies = pkg[type]
  if (target !== null) {
    if (dependencies[dependency]) {
      if (dependency[dependency] !== target)
        logger.log(
          'Setting',
          JSON.stringify(dependency),
          'to',
          JSON.stringify(target),
        )
    } else {
      logger.log('Adding', JSON.stringify(`${dependency}@${target}`))
    }
    dependencies[dependency] = target
  } else if (dependencies[dependency]) {
    logger.log('Removing', JSON.stringify(dependency))
    delete dependencies[dependency]
  }
}

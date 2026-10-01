export interface MakeEnvOptions {
  nodeOptions?: string
  production?: boolean
}

/** The name the environment gives `PATH`, which Windows does not fix to one spelling. */
export const PATH = findPathEnvName()

function findPathEnvName (): string {
  if (process.platform !== 'win32') return 'PATH'
  // windows calls it's path 'Path' usually, but this is not guaranteed.
  let pathEnvName = 'Path'
  for (const envName of Object.keys(process.env)) {
    if (envName.match(/^PATH$/i)) {
      pathEnvName = envName
    }
  }
  return pathEnvName
}

export function makeEnv (data: Record<string, unknown>, opts: MakeEnvOptions, prefix?: string | null, env?: Record<string, string>): Record<string, string> {
  prefix = prefix ?? 'npm_package_'
  if (!env) {
    env = inheritProcessEnv(opts)
  } else if (!Object.prototype.hasOwnProperty.call(data, '_lifecycleEnv')) {
    Object.defineProperty(data, '_lifecycleEnv',
      {
        value: env,
        enumerable: false,
      }
    )
  }

  if (opts.nodeOptions) env.NODE_OPTIONS = opts.nodeOptions

  for (const key in data) {
    if (key.charAt(0) === '_') continue
    const envKey = (prefix + key).replace(/\W/g, '_')
    if (!isExportedPackageField(key, prefix)) continue
    addPackageFieldToEnv(data[key], { envKey, opts, env })
  }

  return env
}

function inheritProcessEnv (opts: MakeEnvOptions): Record<string, string> {
  const env: Record<string, string> = {}
  for (const envName in process.env) {
    if (isInheritedEnvName(envName)) {
      env[envName] = process.env[envName]!
    }
  }

  // express and others respect the NODE_ENV value.
  if (opts.production) env.NODE_ENV = 'production'
  return env
}

/**
 * npm_package_* are regenerated from the package. (npm|pnpm)_config_* auth settings
 * (e.g. _auth, _authToken, _password, //registry/:_authToken) are
 * stripped so they never leak into dependency lifecycle scripts. This
 * mirrors npm's own env-export filter, where config keys starting with
 * _, /, or @ (or containing :_) are treated as private. npm reads the
 * variables case-insensitively, so the filter does too.
 */
function isInheritedEnvName (envName: string): boolean {
  return !envName.match(/^npm_package_/) &&
    !envName.match(/^(?:npm|pnpm)_config_(?:[/@_]|.*:_)/i) &&
    (!envName.match(/^PATH$/i) || envName === PATH)
}

function isExportedPackageField (key: string, prefix: string): boolean {
  return ['name', 'version', 'config', 'engines', 'bin'].includes(key) ||
    prefix.startsWith('npm_package_config_') ||
    prefix.startsWith('npm_package_engines_') ||
    prefix.startsWith('npm_package_bin_')
}

interface PackageFieldEnv {
  envKey: string
  opts: MakeEnvOptions
  env: Record<string, string>
}

function addPackageFieldToEnv (value: unknown, { envKey, opts, env }: PackageFieldEnv): void {
  if (!value || typeof value !== 'object') {
    const stringValue = String(value)
    env[envKey] = stringValue.includes('\n')
      ? JSON.stringify(stringValue)
      : stringValue
    return
  }
  try {
    // quick and dirty detection for cyclical structures
    JSON.stringify(value)
    makeEnv(value as Record<string, unknown>, opts, `${envKey}_`, env)
  } catch {
    // usually these are package objects.
    // just get the path and basic details.
    const packageLike = value as { name?: unknown, version?: unknown, path?: unknown }
    makeEnv(
      { name: packageLike.name, version: packageLike.version, path: packageLike.path },
      opts,
      `${envKey}_`,
      env
    )
  }
}

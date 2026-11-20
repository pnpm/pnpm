'use strict'

const Module = require('module')
const path = require('path')
const { fileURLToPath } = require('url')

const registry = __PNPM_PACKAGE_REGISTRY__.packages
const modulesDir = path.resolve(__dirname, __PNPM_MODULES_DIR__)
const originalResolveFilename = Module._resolveFilename
const topLevel = { name: null, reference: null }

function packageLocation(pkg) {
  return pkg.url.startsWith('file:') ? fileURLToPath(pkg.url) : path.resolve(modulesDir, pkg.url)
}

const locations = Object.entries(registry)
  .map(([id, pkg]) => [id, packageLocation(pkg)])
  .sort((a, b) => b[1].length - a[1].length)

const incomingNames = new Map()
for (const pkg of Object.values(registry)) {
  for (const [name, id] of Object.entries(pkg.dependencies)) {
    if (!incomingNames.has(id)) incomingNames.set(id, name)
  }
}

function locatorFromPackage(id, pkg) {
  if (id === '.') return topLevel
  const selfDependency = Object.entries(pkg.dependencies).find(([, dependencyId]) => dependencyId === id)
  const name = selfDependency?.[0] ?? incomingNames.get(id) ?? id
  const prefix = `${name}@`
  return { name, reference: id.startsWith(prefix) ? id.slice(prefix.length) : id }
}

function locatorKey(locator) {
  return JSON.stringify([locator.name, locator.reference])
}

const locatorById = new Map()
const idByLocator = new Map()
for (const [id, pkg] of Object.entries(registry)) {
  const locator = locatorFromPackage(id, pkg)
  locatorById.set(id, locator)
  idByLocator.set(locatorKey(locator), id)
}

function packageName(request) {
  if (!request || request[0] === '.' || request[0] === '#' || path.isAbsolute(request)) return null
  if (request[0] !== '@') return request.split('/', 1)[0]
  const parts = request.split('/')
  return parts.length > 1 ? `${parts[0]}/${parts[1]}` : request
}

function packageIdForLocation(location) {
  const normalized = path.resolve(location || __dirname)
  return locations.find(([, packagePath]) =>
    normalized === packagePath || normalized.startsWith(`${packagePath}${path.sep}`)
  )?.[0]
}

function moduleForIssuer(issuer) {
  const filename = path.resolve(issuer || __filename)
  const parent = new Module(filename)
  parent.filename = filename
  parent.paths = Module._nodeModulePaths(path.dirname(filename))
  return parent
}

function nativeResolve(request, issuer, options) {
  return originalResolveFilename.call(Module, request, moduleForIssuer(issuer), false, options)
}

function dependencyTarget(alias, dependencyId) {
  const locator = locatorById.get(dependencyId)
  if (locator === undefined) return dependencyId
  if (locator.name === alias) return locator.reference
  return [locator.name, locator.reference]
}

function getPackageInformation(locator) {
  const id = idByLocator.get(locatorKey(locator))
  if (id === undefined) return null
  const pkg = registry[id]
  const location = packageLocation(pkg)
  const packageDependencies = new Map()
  for (const [alias, dependencyId] of Object.entries(pkg.dependencies)) {
    if (id === '.' && dependencyId === id) continue
    packageDependencies.set(alias, dependencyTarget(alias, dependencyId))
  }
  const relativeToModules = path.relative(modulesDir, location)
  return {
    packageLocation: location.endsWith(path.sep) ? location : `${location}${path.sep}`,
    packageDependencies,
    packagePeers: new Set(),
    linkType: relativeToModules === '..' || relativeToModules.startsWith(`..${path.sep}`) || path.isAbsolute(relativeToModules)
      ? 'SOFT'
      : 'HARD',
    discardFromLookup: false,
  }
}

function findPackageLocator(location) {
  const id = packageIdForLocation(location)
  const locator = id === undefined ? undefined : locatorById.get(id)
  return locator === undefined ? null : { ...locator }
}

function undeclaredDependencyError(name, issuer) {
  const error = new Error(`Your application tried to access ${name}, but it isn't declared in your dependencies`)
  error.code = 'MODULE_NOT_FOUND'
  error.pnpCode = 'UNDECLARED_DEPENDENCY'
  error.data = { dependencyName: name, issuer, request: name }
  return error
}

function resolveToUnqualified(request, issuer, options = {}) {
  if (request === 'pnpapi') return __filename
  const name = packageName(request)
  if (name === null) {
    if (request.startsWith('#')) {
      throw new Error('resolveToUnqualified can not handle private import mappings')
    }
    if (path.isAbsolute(request)) return path.normalize(request)
    if (issuer === null || issuer === undefined) {
      throw new Error('The resolveToUnqualified function must be called with a valid issuer when the request is relative')
    }
    const issuerPath = path.resolve(issuer)
    const issuerDir = issuer.endsWith(path.sep) ? issuerPath : path.dirname(issuerPath)
    return path.resolve(issuerDir, request)
  }
  if (options.considerBuiltins !== false && (request.startsWith('node:') || Module.builtinModules.includes(name))) {
    return null
  }
  const ownerId = packageIdForLocation(issuer)
  if (ownerId === undefined) return nativeResolve(request, issuer, options)
  const dependencyId = registry[ownerId].dependencies[name]
  if (dependencyId === undefined || registry[dependencyId] === undefined) {
    throw undeclaredDependencyError(name, issuer)
  }
  const subpath = request.slice(name.length)
  const dependencyLocation = packageLocation(registry[dependencyId])
  const unqualified = path.resolve(dependencyLocation, `.${subpath}`)
  if (unqualified !== dependencyLocation && !unqualified.startsWith(`${dependencyLocation}${path.sep}`)) {
    const error = new Error(`Your application tried to access a path outside ${name}`)
    error.code = 'MODULE_NOT_FOUND'
    throw error
  }
  return unqualified
}

function resolveUnqualified(unqualified, options) {
  return nativeResolve(unqualified, unqualified, options)
}

function resolveRequest(request, issuer, options = {}) {
  if (request.startsWith('#')) return nativeResolve(request, issuer, options)
  const unqualified = resolveToUnqualified(request, issuer, options)
  if (unqualified === null) return null
  return resolveUnqualified(unqualified, options)
}

function getAllLocators() {
  return [...locatorById.values()]
    .filter((locator) => locator.name !== null)
    .map((locator) => ({ ...locator }))
}

function setup() {
  if (Module._resolveFilename.__pnpmPnp) return
  const hooked = function (request, parent, isMain, options) {
    if (request === 'pnpapi') return __filename
    const name = packageName(request)
    if (name === null || request.startsWith('node:') || Module.builtinModules.includes(name)) {
      return originalResolveFilename.call(this, request, parent, isMain, options)
    }
    const issuer = parent?.filename ?? __filename
    const unqualified = resolveToUnqualified(request, issuer)
    return originalResolveFilename.call(this, unqualified, parent, isMain, options)
  }
  hooked.__pnpmPnp = true
  Module._resolveFilename = hooked
  Module.findPnpApi = (lookupSource) => {
    if (lookupSource === undefined) return api
    return findPackageLocator(lookupSource) === null ? undefined : api
  }
}

const api = {
  VERSIONS: { std: 3, resolveVirtual: 1, getAllLocators: 1 },
  topLevel,
  getLocator: (name, referencish) => Array.isArray(referencish)
    ? { name: referencish[0], reference: referencish[1] }
    : { name, reference: referencish },
  getDependencyTreeRoots: () => [],
  getAllLocators,
  getPackageInformation,
  findPackageLocator,
  resolveToUnqualified,
  resolveUnqualified,
  resolveRequest,
  resolveVirtual: () => null,
  setup,
}

module.exports = api
setup()

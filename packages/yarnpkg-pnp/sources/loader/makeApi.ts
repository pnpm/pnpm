import {ppath, Filename}                                                                                    from '@yarnpkg/fslib';
import {FakeFS, NativePath, PortablePath, VirtualFS, npath}                                                 from '@yarnpkg/fslib';
import {Module}                                                                                             from 'module';

import {PackageInformation, PackageLocator, PnpApi, RuntimeState, PhysicalPackageLocator, DependencyTarget} from '../types';

import {ErrorCode, makeError, getPathForDisplay}                                                            from './internalTools';

export type MakeApiOptions = {
  allowDebug?: boolean,
  compatibilityMode?: boolean,
  fakeFs: FakeFS<PortablePath>,
  pnpapiResolution: NativePath,
};

export type ResolveToUnqualifiedOptions = {
  considerBuiltins?: boolean,
};

export type ResolveUnqualifiedOptions = {
  extensions?: Array<string>,
};

export type ResolveRequestOptions =
  ResolveToUnqualifiedOptions &
  ResolveUnqualifiedOptions;

export function makeApi(runtimeState: RuntimeState, opts: MakeApiOptions): PnpApi {
  const alwaysWarnOnFallback = Number(process.env.PNP_ALWAYS_WARN_ON_FALLBACK) > 0;
  const debugLevel = Number(process.env.PNP_DEBUG_LEVEL);

  // @ts-expect-error
  const builtinModules = new Set(Module.builtinModules || Object.keys(process.binding(`natives`)));

  // Splits a require request into its components, or return null if the request is a file path
  const pathRegExp = /^(?![a-zA-Z]:[\\/]|\\\\|\.{0,2}(?:\/|$))((?:@[^/]+\/)?[^/]+)\/*(.*|)$/;

  // Matches if the path starts with a valid path qualifier (./, ../, /)
  // eslint-disable-next-line no-unused-vars
  const isStrictRegExp = /^(\/|\.{1,2}(\/|$))/;

  // Matches if the path must point to a directory (ie ends with /)
  const isDirRegExp = /\/$/;

  // We only instantiate one of those so that we can use strict-equal comparisons
  const topLevelLocator = {name: null, reference: null};

  // Used for compatibility purposes - cf setupCompatibilityLayer
  const fallbackLocators: Array<PackageLocator> = [];

  // To avoid emitting the same warning multiple times
  const emittedWarnings = new Set<string>();

  if (runtimeState.enableTopLevelFallback === true)
    fallbackLocators.push(topLevelLocator);

  if (opts.compatibilityMode !== false) {
    // ESLint currently doesn't have any portable way for shared configs to
    // specify their own plugins that should be used (cf issue #10125). This
    // will likely get fixed at some point but it'll take time, so in the
    // meantime we'll just add additional fallback entries for common shared
    // configs.

    // Similarly, Gatsby generates files within the `public` folder located
    // within the project, but doesn't pre-resolve the `require` calls to use
    // its own dependencies. Meaning that when PnP see a file from the `public`
    // folder making a require, it thinks that your project forgot to list one
    // of your dependencies.

    for (const name of [`react-scripts`, `gatsby`]) {
      const packageStore = runtimeState.packageRegistry.get(name);
      if (packageStore) {
        for (const reference of packageStore.keys()) {
          if (reference === null) {
            throw new Error(`Assertion failed: This reference shouldn't be null`);
          } else {
            fallbackLocators.push({name, reference});
          }
        }
      }
    }
  }

  /**
   * The setup code will be injected here. The tables listed below are guaranteed to be filled after the call to
   * the $$DYNAMICALLY_GENERATED_CODE function.
   */

  const {
    ignorePattern,
    packageRegistry,
    packageLocatorsByLocations,
    packageLocationLengths,
  } = runtimeState as RuntimeState;

  /**
   * Allows to print useful logs just be setting a value in the environment
   */

  function makeLogEntry(name: string, args: Array<any>) {
    return {
      fn: name,
      args,
      error: null as Error | null,
      result: null as any,
    };
  }

  function maybeLog(name: string, fn: any): any {
    if (opts.allowDebug === false)
      return fn;

    if (Number.isFinite(debugLevel)) {
      if (debugLevel >= 2) {
        return (...args: Array<any>) => {
          const logEntry = makeLogEntry(name, args);

          try {
            return logEntry.result = fn(...args);
          } catch (error) {
            throw logEntry.error = error;
          } finally {
            console.trace(logEntry);
          }
        };
      } else if (debugLevel >= 1) {
        return (...args: Array<any>) => {
          try {
            return fn(...args);
          } catch (error) {
            const logEntry = makeLogEntry(name, args);
            logEntry.error = error;
            console.trace(logEntry);
            throw error;
          }
        };
      }
    }

    return fn;
  }

  /**
   * Returns information about a package in a safe way (will throw if they cannot be retrieved)
   */

  function getPackageInformationSafe(packageLocator: PackageLocator): PackageInformation<PortablePath> {
    const packageInformation = getPackageInformation(packageLocator);

    if (!packageInformation) {
      throw makeError(
        ErrorCode.INTERNAL,
        `Couldn't find a matching entry in the dependency tree for the specified parent (this is probably an internal error)`,
      );
    }

    return packageInformation;
  }

  /**
   * Returns whether the specified locator is a dependency tree root (in which case it's part of the project) or not
   */
  function isDependencyTreeRoot(packageLocator: PackageLocator) {
    if (packageLocator.name === null)
      return true;

    for (const dependencyTreeRoot of runtimeState.dependencyTreeRoots)
      if (dependencyTreeRoot.name === packageLocator.name && dependencyTreeRoot.reference === packageLocator.reference)
        return true;

    return false;
  }

  /**
   * Implements the node resolution for folder access and extension selection
   */

  function applyNodeExtensionResolution(unqualifiedPath: PortablePath, candidates: Array<PortablePath>, {extensions}: {extensions: Array<string>}): PortablePath | null {
    let stat;

    try {
      candidates.push(unqualifiedPath);
      stat = opts.fakeFs.statSync(unqualifiedPath);
    } catch (error) {}

    // If the file exists and is a file, we can stop right there

    if (stat && !stat.isDirectory())
      return opts.fakeFs.realpathSync(unqualifiedPath);

    // If the file is a directory, we must check if it contains a package.json with a "main" entry

    if (stat && stat.isDirectory()) {
      let pkgJson;

      try {
        pkgJson = JSON.parse(opts.fakeFs.readFileSync(ppath.join(unqualifiedPath, `package.json` as Filename), `utf8`));
      } catch (error) {}

      let nextUnqualifiedPath;

      if (pkgJson && pkgJson.main)
        nextUnqualifiedPath = ppath.resolve(unqualifiedPath, pkgJson.main);

      // If the "main" field changed the path, we start again from this new location

      if (nextUnqualifiedPath && nextUnqualifiedPath !== unqualifiedPath) {
        const resolution = applyNodeExtensionResolution(nextUnqualifiedPath, candidates, {extensions});

        if (resolution !== null) {
          return resolution;
        }
      }
    }

    // Otherwise we check if we find a file that match one of the supported extensions

    for (let i = 0, length = extensions.length; i < length; i++) {
      const candidateFile = `${unqualifiedPath}${extensions[i]}` as PortablePath;
      candidates.push(candidateFile);
      if (opts.fakeFs.existsSync(candidateFile)) {
        return candidateFile;
      }
    }

    // Otherwise, we check if the path is a folder - in such a case, we try to use its index

    if (stat && stat.isDirectory()) {
      for (let i = 0, length = extensions.length; i < length; i++) {
        const candidateFile = ppath.format({dir: unqualifiedPath, name: `index` as Filename, ext: extensions[i]});
        candidates.push(candidateFile);
        if (opts.fakeFs.existsSync(candidateFile)) {
          return candidateFile;
        }
      }
    }

    // Otherwise there's nothing else we can do :(

    return null;
  }

  /**
   * This function creates fake modules that can be used with the _resolveFilename function.
   * Ideally it would be nice to be able to avoid this, since it causes useless allocations
   * and cannot be cached efficiently (we recompute the nodeModulePaths every time).
   *
   * Fortunately, this should only affect the fallback, and there hopefully shouldn't have a
   * lot of them.
   */

  function makeFakeModule(path: NativePath): NodeModule {
    // @ts-expect-error
    const fakeModule = new Module(path, null);
    fakeModule.filename = path;
    fakeModule.paths = Module._nodeModulePaths(path);
    return fakeModule;
  }

  /**
   * Forward the resolution to the next resolver (usually the native one)
   */

  function callNativeResolution(request: PortablePath, issuer: PortablePath): NativePath | false {
    if (issuer.endsWith(`/`))
      issuer = ppath.join(issuer, `internal.js` as Filename);

    // Since we would need to create a fake module anyway (to call _resolveLookupPath that
    // would give us the paths to give to _resolveFilename), we can as well not use
    // the {paths} option at all, since it internally makes _resolveFilename create another
    // fake module anyway.
    return Module._resolveFilename(npath.fromPortablePath(request), makeFakeModule(npath.fromPortablePath(issuer)), false, {plugnplay: false});
  }

  /**
   *
   */

  function isPathIgnored(path: PortablePath) {
    if (ignorePattern === null)
      return false;

    const subPath = ppath.contains(runtimeState.basePath, path);
    if (subPath === null)
      return false;

    if (ignorePattern.test(subPath.replace(/\/$/, ``))) {
      return true;
    } else {
      return false;
    }
  }

  /**
   * This key indicates which version of the standard is implemented by this resolver. The `std` key is the
   * Plug'n'Play standard, and any other key are third-party extensions. Third-party extensions are not allowed
   * to override the standard, and can only offer new methods.
   *
   * If a new version of the Plug'n'Play standard is released and some extensions conflict with newly added
   * functions, they'll just have to fix the conflicts and bump their own version number.
   */

  const VERSIONS = {std: 3, resolveVirtual: 1, getAllLocators: 1};

  /**
   * We export a special symbol for easy access to the top level locator.
   */

  const topLevel = topLevelLocator;

  /**
   * Gets the package information for a given locator. Returns null if they cannot be retrieved.
   */

  function getPackageInformation({name, reference}: PackageLocator): PackageInformation<PortablePath> | null {
    const packageInformationStore = packageRegistry.get(name);
    if (!packageInformationStore)
      return null;

    const packageInformation = packageInformationStore.get(reference);
    if (!packageInformation)
      return null;

    return packageInformation;
  }

  /**
   * Find all packages that depend on the specified one.
   *
   * Note: This is a private function; we expect consumers to implement it
   * themselves. We keep it that way because this implementation isn't
   * optimized at all, since we only need it when printing errors.
   */

  function findPackageDependents({name, reference}: PhysicalPackageLocator): Array<PhysicalPackageLocator> {
    const dependents: Array<PhysicalPackageLocator> = [];

    for (const [dependentName, packageInformationStore] of packageRegistry) {
      if (dependentName === null)
        continue;

      for (const [dependentReference, packageInformation] of packageInformationStore) {
        if (dependentReference === null)
          continue;

        const dependencyReference = packageInformation.packageDependencies.get(name);
        if (dependencyReference !== reference)
          continue;

        // Don't forget that all packages depend on themselves
        if (dependentName === name && dependentReference === reference)
          continue;

        dependents.push({
          name: dependentName,
          reference: dependentReference,
        });
      }
    }

    return dependents;
  }

  /**
   * Find all packages that broke the peer dependency on X, starting from Y.
   *
   * Note: This is a private function; we expect consumers to implement it
   * themselves. We keep it that way because this implementation isn't
   * optimized at all, since we only need it when printing errors.
   */

  function findBrokenPeerDependencies(dependency: string, initialPackage: PhysicalPackageLocator): Array<PhysicalPackageLocator> {
    const brokenPackages = new Map<string, Set<string>>();

    const alreadyVisited = new Set<string>();

    const traversal = (currentPackage: PhysicalPackageLocator) => {
      const identifier = JSON.stringify(currentPackage.name);
      if (alreadyVisited.has(identifier))
        return;

      alreadyVisited.add(identifier);

      const dependents = findPackageDependents(currentPackage);

      for (const dependent of dependents) {
        const dependentInformation = getPackageInformationSafe(dependent);

        if (dependentInformation.packagePeers.has(dependency)) {
          traversal(dependent);
        } else {
          let brokenSet = brokenPackages.get(dependent.name);
          if (typeof brokenSet === `undefined`)
            brokenPackages.set(dependent.name, brokenSet = new Set());

          brokenSet.add(dependent.reference);
        }
      }
    };

    traversal(initialPackage);

    const brokenList: Array<PhysicalPackageLocator> = [];

    for (const name of [...brokenPackages.keys()].sort())
      for (const reference of [...brokenPackages.get(name)!].sort())
        brokenList.push({name, reference});

    return brokenList;
  }

  /**
   * Finds the package locator that owns the specified path. If none is found, returns null instead.
   */

  function findPackageLocator(location: PortablePath): PhysicalPackageLocator | null {
    if (isPathIgnored(location))
      return null;

    let relativeLocation = ppath.relative(runtimeState.basePath, location);

    if (!relativeLocation.match(isStrictRegExp))
      relativeLocation = `./${relativeLocation}` as PortablePath;

    if (!relativeLocation.endsWith(`/`))
      relativeLocation = `${relativeLocation}/` as PortablePath;

    let from = 0;

    // If someone wants to use a binary search to go from O(n) to O(log n), be my guest
    while (from < packageLocationLengths.length && packageLocationLengths[from] > relativeLocation.length)
      from += 1;

    for (let t = from; t < packageLocationLengths.length; ++t) {
      const locator = packageLocatorsByLocations.get(relativeLocation.substr(0, packageLocationLengths[t]) as PortablePath);
      if (typeof locator === `undefined`)
        continue;

      // Ensures that the returned locator isn't a blacklisted one.
      //
      // Blacklisted packages are packages that cannot be used because their dependencies cannot be deduced. This only
      // happens with peer dependencies, which effectively have different sets of dependencies depending on their
      // parents.
      //
      // In order to deambiguate those different sets of dependencies, the Yarn implementation of PnP will generate a
      // symlink for each combination of <package name>/<package version>/<dependent package> it will find, and will
      // blacklist the target of those symlinks. By doing this, we ensure that files loaded through a specific path
      // will always have the same set of dependencies, provided the symlinks are correctly preserved.
      //
      // Unfortunately, some tools do not preserve them, and when it happens PnP isn't able anymore to deduce the set of
      // dependencies based on the path of the file that makes the require calls. But since we've blacklisted those
      // paths, we're able to print a more helpful error message that points out that a third-party package is doing
      // something incompatible!

      if (locator === null) {
        const locationForDisplay = getPathForDisplay(location);
        throw makeError(
          ErrorCode.BLACKLISTED,
          `A forbidden path has been used in the package resolution process - this is usually caused by one of your tools calling 'fs.realpath' on the return value of 'require.resolve'. Since we need to use symlinks to simultaneously provide valid filesystem paths and disambiguate peer dependencies, they must be passed untransformed to 'require'.\n\nForbidden path: ${locationForDisplay}`,
          {location: locationForDisplay},
        );
      }

      return locator;
    }

    return null;
  }

  /**
   * Transforms a request (what's typically passed as argument to the require function) into an unqualified path.
   * This path is called "unqualified" because it only changes the package name to the package location on the disk,
   * which means that the end result still cannot be directly accessed (for example, it doesn't try to resolve the
   * file extension, or to resolve directories to their "index.js" content). Use the "resolveUnqualified" function
   * to convert them to fully-qualified paths, or just use "resolveRequest" that do both operations in one go.
   *
   * Note that it is extremely important that the `issuer` path ends with a forward slash if the issuer is to be
   * treated as a folder (ie. "/tmp/foo/" rather than "/tmp/foo" if "foo" is a directory). Otherwise relative
   * imports won't be computed correctly (they'll get resolved relative to "/tmp/" instead of "/tmp/foo/").
   */

  function resolveToUnqualified(request: PortablePath, issuer: PortablePath | null, {considerBuiltins = true}: ResolveToUnqualifiedOptions = {}): PortablePath | null {
    // The 'pnpapi' request is reserved and will always return the path to the PnP file, from everywhere

    if (request === `pnpapi`)
      return npath.toPortablePath(opts.pnpapiResolution);

    // Bailout if the request is a native module

    if (considerBuiltins && builtinModules.has(request))
      return null;

    const requestForDisplay = getPathForDisplay(request);
    const issuerForDisplay = issuer && getPathForDisplay(issuer);

    // We allow disabling the pnp resolution for some subpaths.
    // This is because some projects, often legacy, contain multiple
    // levels of dependencies (ie. a yarn.lock inside a subfolder of
    // a yarn.lock). This is typically solved using workspaces, but
    // not all of them have been converted already.

    if (issuer && isPathIgnored(issuer)) {
      // Absolute paths that seem to belong to a PnP tree are still
      // handled by our runtime even if the issuer isn't. This is
      // because the native Node resolution uses a special version
      // of the `stat` syscall which would otherwise bypass the
      // filesystem layer we require to access the files.

      if (!ppath.isAbsolute(request) || findPackageLocator(request) === null) {
        const result = callNativeResolution(request, issuer);

        if (result === false) {
          throw makeError(
            ErrorCode.BUILTIN_NODE_RESOLUTION_FAILED,
            `The builtin node resolution algorithm was unable to resolve the requested module (it didn't go through the pnp resolver because the issuer was explicitely ignored by the regexp)\n\nRequire request: "${requestForDisplay}"\nRequired by: ${issuerForDisplay}\n`,
            {request: requestForDisplay, issuer: issuerForDisplay},
          );
        }

        return npath.toPortablePath(result);
      }
    }

    let unqualifiedPath: PortablePath;

    // If the request is a relative or absolute path, we just return it normalized

    const dependencyNameMatch = request.match(pathRegExp);

    if (!dependencyNameMatch) {
      if (ppath.isAbsolute(request)) {
        unqualifiedPath = ppath.normalize(request);
      } else {
        if (!issuer) {
          throw makeError(
            ErrorCode.API_ERROR,
            `The resolveToUnqualified function must be called with a valid issuer when the path isn't a builtin nor absolute`,
            {request: requestForDisplay, issuer: issuerForDisplay},
          );
        }

        // We use ppath.join instead of ppath.resolve because:
        // 1) The request is a relative path in this branch
        // 2) ppath.join preserves trailing slashes

        const absoluteIssuer = ppath.resolve(issuer);
        if (issuer.match(isDirRegExp)) {
          unqualifiedPath = ppath.normalize(ppath.join(absoluteIssuer, request));
        } else {
          unqualifiedPath = ppath.normalize(ppath.join(ppath.dirname(absoluteIssuer), request));
        }
      }

      // No need to use the return value; we just want to check the blacklist status
      findPackageLocator(unqualifiedPath);
    } else {
      // Things are more hairy if it's a package require - we then need to figure out which package is needed, and in
      // particular the exact version for the given location on the dependency tree

      if (!issuer) {
        throw makeError(
          ErrorCode.API_ERROR,
          `The resolveToUnqualified function must be called with a valid issuer when the path isn't a builtin nor absolute`,
          {request: requestForDisplay, issuer: issuerForDisplay},
        );
      }

      const [, dependencyName, subPath] = dependencyNameMatch as [unknown, string, PortablePath];

      const issuerLocator = findPackageLocator(issuer);

      // If the issuer file doesn't seem to be owned by a package managed through pnp, then we resort to using the next
      // resolution algorithm in the chain, usually the native Node resolution one

      if (!issuerLocator) {
        const result = callNativeResolution(request, issuer);

        if (result === false) {
          throw makeError(
            ErrorCode.BUILTIN_NODE_RESOLUTION_FAILED,
            `The builtin node resolution algorithm was unable to resolve the requested module (it didn't go through the pnp resolver because the issuer doesn't seem to be part of the Yarn-managed dependency tree).\n\nRequire path: "${requestForDisplay}"\nRequired by: ${issuerForDisplay}\n`,
            {request: requestForDisplay, issuer: issuerForDisplay},
          );
        }

        return npath.toPortablePath(result);
      }

      const issuerInformation = getPackageInformationSafe(issuerLocator);

      // We obtain the dependency reference in regard to the package that request it

      let dependencyReference = issuerInformation.packageDependencies.get(dependencyName);
      let fallbackReference: DependencyTarget | null = null;

      // If we can't find it, we check if we can potentially load it from the packages that have been defined as potential fallbacks.
      // It's a bit of a hack, but it improves compatibility with the existing Node ecosystem. Hopefully we should eventually be able
      // to kill this logic and become stricter once pnp gets enough traction and the affected packages fix themselves.

      if (dependencyReference == null) {
        if (issuerLocator.name !== null) {
          // To allow programs to become gradually stricter, starting from the v2 we enforce that workspaces cannot depend on fallbacks.
          // This works by having a list containing all their locators, and checking when a fallback is required whether it's one of them.
          const exclusionEntry = runtimeState.fallbackExclusionList.get(issuerLocator.name);
          const canUseFallbacks = !exclusionEntry || !exclusionEntry.has(issuerLocator.reference);

          if (canUseFallbacks) {
            for (let t = 0, T = fallbackLocators.length; t < T; ++t) {
              const fallbackInformation = getPackageInformationSafe(fallbackLocators[t]);
              const reference = fallbackInformation.packageDependencies.get(dependencyName);

              if (reference == null)
                continue;

              if (alwaysWarnOnFallback)
                fallbackReference = reference;
              else
                dependencyReference = reference;

              break;
            }

            if (runtimeState.enableTopLevelFallback) {
              if (dependencyReference == null && fallbackReference === null) {
                const reference = runtimeState.fallbackPool.get(dependencyName);
                if (reference != null) {
                  fallbackReference = reference;
                }
              }
            }
          }
        }
      }

      // If we can't find the path, and if the package making the request is the top-level, we can offer nicer error messages

      let error: Error | null = null;

      if (dependencyReference === null) {
        if (isDependencyTreeRoot(issuerLocator)) {
          error = makeError(
            ErrorCode.MISSING_PEER_DEPENDENCY,
            `Your application tried to access ${dependencyName} (a peer dependency); this isn't allowed as there is no ancestor to satisfy the requirement. Use a devDependency if needed.\n\nRequired package: ${dependencyName} (via "${requestForDisplay}")\nRequired by: ${issuerForDisplay}\n`,
            {request: requestForDisplay, issuer: issuerForDisplay, dependencyName},
          );
        } else {
          const brokenAncestors = findBrokenPeerDependencies(dependencyName, issuerLocator);
          if (brokenAncestors.every(ancestor => isDependencyTreeRoot(ancestor))) {
            error = makeError(
              ErrorCode.MISSING_PEER_DEPENDENCY,
              `${issuerLocator.name} tried to access ${dependencyName} (a peer dependency) but it isn't provided by your application; this makes the require call ambiguous and unsound.\n\nRequired package: ${dependencyName} (via "${requestForDisplay}")\nRequired by: ${issuerLocator.name}@${issuerLocator.reference} (via ${issuerForDisplay})\n${brokenAncestors.map(ancestorLocator => `Ancestor breaking the chain: ${ancestorLocator.name}@${ancestorLocator.reference}\n`).join(``)}\n`,
              {request: requestForDisplay, issuer: issuerForDisplay, issuerLocator: Object.assign({}, issuerLocator), dependencyName, brokenAncestors},
            );
          } else {
            error = makeError(
              ErrorCode.MISSING_PEER_DEPENDENCY,
              `${issuerLocator.name} tried to access ${dependencyName} (a peer dependency) but it isn't provided by its ancestors; this makes the require call ambiguous and unsound.\n\nRequired package: ${dependencyName} (via "${requestForDisplay}")\nRequired by: ${issuerLocator.name}@${issuerLocator.reference} (via ${issuerForDisplay})\n${brokenAncestors.map(ancestorLocator => `Ancestor breaking the chain: ${ancestorLocator.name}@${ancestorLocator.reference}\n`).join(``)}\n`,
              {request: requestForDisplay, issuer: issuerForDisplay, issuerLocator: Object.assign({}, issuerLocator), dependencyName, brokenAncestors},
            );
          }
        }
      } else if (dependencyReference === undefined) {
        if (!considerBuiltins && builtinModules.has(request)) {
          if (isDependencyTreeRoot(issuerLocator)) {
            error = makeError(
              ErrorCode.UNDECLARED_DEPENDENCY,
              `Your application tried to access ${dependencyName}. While this module is usually interpreted as a Node builtin, your resolver is running inside a non-Node resolution context where such builtins are ignored. Since ${dependencyName} isn't otherwise declared in your dependencies, this makes the require call ambiguous and unsound.\n\nRequired package: ${dependencyName} (via "${requestForDisplay}")\nRequired by: ${issuerForDisplay}\n`,
              {request: requestForDisplay, issuer: issuerForDisplay, dependencyName},
            );
          } else {
            error = makeError(
              ErrorCode.UNDECLARED_DEPENDENCY,
              `${issuerLocator.name} tried to access ${dependencyName}. While this module is usually interpreted as a Node builtin, your resolver is running inside a non-Node resolution context where such builtins are ignored. Since ${dependencyName} isn't otherwise declared in ${issuerLocator.name}'s dependencies, this makes the require call ambiguous and unsound.\n\nRequired package: ${dependencyName} (via "${requestForDisplay}")\nRequired by: ${issuerForDisplay}\n`,
              {request: requestForDisplay, issuer: issuerForDisplay, issuerLocator: Object.assign({}, issuerLocator), dependencyName},
            );
          }
        } else {
          if (isDependencyTreeRoot(issuerLocator)) {
            error = makeError(
              ErrorCode.UNDECLARED_DEPENDENCY,
              `Your application tried to access ${dependencyName}, but it isn't declared in your dependencies; this makes the require call ambiguous and unsound.\n\nRequired package: ${dependencyName} (via "${requestForDisplay}")\nRequired by: ${issuerForDisplay}\n`,
              {request: requestForDisplay, issuer: issuerForDisplay, dependencyName},
            );
          } else {
            error = makeError(
              ErrorCode.UNDECLARED_DEPENDENCY,
              `${issuerLocator.name} tried to access ${dependencyName}, but it isn't declared in its dependencies; this makes the require call ambiguous and unsound.\n\nRequired package: ${dependencyName} (via "${requestForDisplay}")\nRequired by: ${issuerLocator.name}@${issuerLocator.reference} (via ${issuerForDisplay})\n`,
              {request: requestForDisplay, issuer: issuerForDisplay, issuerLocator: Object.assign({}, issuerLocator), dependencyName},
            );
          }
        }
      }

      if (dependencyReference == null) {
        if (fallbackReference === null || error === null)
          throw error || new Error(`Assertion failed: Expected an error to have been set`);

        dependencyReference = fallbackReference;

        const message = error.message.replace(/\n.*/g, ``);
        error.message = message;

        if (!emittedWarnings.has(message) && debugLevel !== 0) {
          emittedWarnings.add(message);
          process.emitWarning(error);
        }
      }

      // We need to check that the package exists on the filesystem, because it might not have been installed

      const dependencyLocator = Array.isArray(dependencyReference)
        ? {name: dependencyReference[0], reference: dependencyReference[1]}
        : {name: dependencyName, reference: dependencyReference};

      const dependencyInformation = getPackageInformationSafe(dependencyLocator);

      if (!dependencyInformation.packageLocation) {
        throw makeError(
          ErrorCode.MISSING_DEPENDENCY,
          `A dependency seems valid but didn't get installed for some reason. This might be caused by a partial install, such as dev vs prod.\n\nRequired package: ${dependencyLocator.name}@${dependencyLocator.reference} (via "${requestForDisplay}")\nRequired by: ${issuerLocator.name}@${issuerLocator.reference} (via ${issuerForDisplay})\n`,
          {request: requestForDisplay, issuer: issuerForDisplay, dependencyLocator: Object.assign({}, dependencyLocator)},
        );
      }

      // Now that we know which package we should resolve to, we only have to find out the file location

      // packageLocation is always absolute as it's returned by getPackageInformationSafe
      const dependencyLocation = dependencyInformation.packageLocation;

      if (subPath) {
        // We use ppath.join instead of ppath.resolve because:
        // 1) subPath is always a relative path
        // 2) ppath.join preserves trailing slashes
        unqualifiedPath = ppath.join(dependencyLocation, subPath);
      } else {
        unqualifiedPath = dependencyLocation;
      }
    }

    return ppath.normalize(unqualifiedPath);
  }

  /**
   * Transforms an unqualified path into a qualified path by using the Node resolution algorithm (which automatically
   * appends ".js" / ".json", and transforms directory accesses into "index.js").
   */

  function resolveUnqualified(unqualifiedPath: PortablePath, {extensions = Object.keys(Module._extensions)}: ResolveUnqualifiedOptions = {}): PortablePath {
    const candidates: Array<PortablePath> = [];
    const qualifiedPath = applyNodeExtensionResolution(unqualifiedPath, candidates, {extensions});

    if (qualifiedPath) {
      return ppath.normalize(qualifiedPath);
    } else {
      const unqualifiedPathForDisplay = getPathForDisplay(unqualifiedPath);
      throw makeError(
        ErrorCode.QUALIFIED_PATH_RESOLUTION_FAILED,
        `Qualified path resolution failed - none of the candidates can be found on the disk.\n\nSource path: ${unqualifiedPathForDisplay}\n${candidates.map(candidate => `Rejected candidate: ${getPathForDisplay(candidate)}\n`).join(``)}`,
        {unqualifiedPath: unqualifiedPathForDisplay},
      );
    }
  }

  /**
   * Transforms a request into a fully qualified path.
   *
   * Note that it is extremely important that the `issuer` path ends with a forward slash if the issuer is to be
   * treated as a folder (ie. "/tmp/foo/" rather than "/tmp/foo" if "foo" is a directory). Otherwise relative
   * imports won't be computed correctly (they'll get resolved relative to "/tmp/" instead of "/tmp/foo/").
   */

  function resolveRequest(request: PortablePath, issuer: PortablePath | null, {considerBuiltins, extensions}: ResolveRequestOptions = {}): PortablePath | null {
    const unqualifiedPath = resolveToUnqualified(request, issuer, {considerBuiltins});

    if (unqualifiedPath === null)
      return null;

    try {
      return resolveUnqualified(unqualifiedPath, {extensions});
    } catch (resolutionError) {
      if (resolutionError.pnpCode === `QUALIFIED_PATH_RESOLUTION_FAILED`)
        Object.assign(resolutionError.data, {request: getPathForDisplay(request), issuer: issuer && getPathForDisplay(issuer)});

      throw resolutionError;
    }
  }

  function resolveVirtual(request: PortablePath) {
    const normalized = ppath.normalize(request);
    const resolved = VirtualFS.resolveVirtual(normalized);

    return resolved !== normalized ? resolved : null;
  }

  return {
    VERSIONS,
    topLevel,

    getLocator: (name: string, referencish: [string, string] | string): PhysicalPackageLocator => {
      if (Array.isArray(referencish)) {
        return {name: referencish[0], reference: referencish[1]};
      } else {
        return {name, reference: referencish};
      }
    },

    getDependencyTreeRoots: () => {
      return [...runtimeState.dependencyTreeRoots];
    },

    getAllLocators() {
      const locators: Array<PhysicalPackageLocator> = [];

      for (const [name, entry] of packageRegistry)
        for (const reference of entry.keys())
          if (name !== null && reference !== null)
            locators.push({name, reference});

      return locators;
    },

    getPackageInformation: (locator: PackageLocator) => {
      const info = getPackageInformation(locator);

      if (info === null)
        return null;

      const packageLocation = npath.fromPortablePath(info.packageLocation);
      const nativeInfo = {...info, packageLocation};

      return nativeInfo;
    },

    findPackageLocator: (path: string) => {
      return findPackageLocator(npath.toPortablePath(path));
    },

    resolveToUnqualified: maybeLog(`resolveToUnqualified`, (request: NativePath, issuer: NativePath | null, opts?: ResolveToUnqualifiedOptions) => {
      const portableIssuer = issuer !== null ?  npath.toPortablePath(issuer) : null;

      const resolution = resolveToUnqualified(npath.toPortablePath(request), portableIssuer, opts);
      if (resolution === null)
        return null;

      return npath.fromPortablePath(resolution);
    }),

    resolveUnqualified: maybeLog(`resolveUnqualified`, (unqualifiedPath: NativePath, opts?: ResolveUnqualifiedOptions) => {
      return npath.fromPortablePath(resolveUnqualified(npath.toPortablePath(unqualifiedPath), opts));
    }),

    resolveRequest: maybeLog(`resolveRequest`, (request: NativePath, issuer: NativePath | null, opts?: ResolveRequestOptions) => {
      const portableIssuer = issuer !== null ? npath.toPortablePath(issuer) : null;

      const resolution = resolveRequest(npath.toPortablePath(request), portableIssuer, opts);
      if (resolution === null)
        return null;

      return npath.fromPortablePath(resolution);
    }),

    resolveVirtual: maybeLog(`resolveVirtual`, (path: NativePath) => {
      const result = resolveVirtual(npath.toPortablePath(path));

      if (result !== null) {
        return npath.fromPortablePath(result);
      } else {
        return null;
      }
    }),
  };
}

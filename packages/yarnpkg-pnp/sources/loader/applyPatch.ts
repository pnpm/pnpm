import {FakeFS, PosixFS, npath, patchFs, PortablePath, Filename, NativePath} from '@yarnpkg/fslib';
import fs                                                                    from 'fs';
import {Module}                                                              from 'module';
import {URL, fileURLToPath}                                                  from 'url';

import {PnpApi}                                                              from '../types';

import {ErrorCode, makeError, getIssuerModule}                               from './internalTools';
import {Manager}                                                             from './makeManager';

export type ApplyPatchOptions = {
  fakeFs: FakeFS<PortablePath>,
  manager: Manager,
};

export function applyPatch(pnpapi: PnpApi, opts: ApplyPatchOptions) {
  // @ts-expect-error
  const builtinModules = new Set(Module.builtinModules || Object.keys(process.binding(`natives`)));

  /**
   * The cache that will be used for all accesses occuring outside of a PnP context.
   */

  const defaultCache: NodeJS.NodeRequireCache = {};

  /**
   * Used to disable the resolution hooks (for when we want to fallback to the previous resolution - we then need
   * a way to "reset" the environment temporarily)
   */

  let enableNativeHooks = true;

  // @ts-expect-error
  process.versions.pnp = String(pnpapi.VERSIONS.std);

  const moduleExports = require(`module`);

  moduleExports.findPnpApi = (lookupSource: URL | NativePath) => {
    const lookupPath = lookupSource instanceof URL
      ? fileURLToPath(lookupSource)
      : lookupSource;

    const apiPath = opts.manager.findApiPathFor(lookupPath);
    if (apiPath === null)
      return null;

    const apiEntry = opts.manager.getApiEntry(apiPath, true);
    return apiEntry.instance;
  };

  function getRequireStack(parent: Module | null | undefined) {
    const requireStack = [];

    for (let cursor = parent; cursor; cursor = cursor.parent)
      requireStack.push(cursor.filename || cursor.id);

    return requireStack;
  }

  // A small note: we don't replace the cache here (and instead use the native one). This is an effort to not
  // break code similar to "delete require.cache[require.resolve(FOO)]", where FOO is a package located outside
  // of the Yarn dependency tree. In this case, we defer the load to the native loader. If we were to replace the
  // cache by our own, the native loader would populate its own cache, which wouldn't be exposed anymore, so the
  // delete call would be broken.

  const originalModuleLoad = Module._load;

  Module._load = function(request: string, parent: NodeModule | null | undefined, isMain: boolean) {
    if (!enableNativeHooks)
      return originalModuleLoad.call(Module, request, parent, isMain);

    // Builtins are managed by the regular Node loader

    if (builtinModules.has(request)) {
      try {
        enableNativeHooks = false;
        return originalModuleLoad.call(Module, request, parent, isMain);
      } finally {
        enableNativeHooks = true;
      }
    }

    const parentApiPath = opts.manager.getApiPathFromParent(parent);

    const parentApi = parentApiPath !== null
      ? opts.manager.getApiEntry(parentApiPath, true).instance
      : null;

    // Requests that aren't covered by the PnP runtime goes through the
    // parent `_load` implementation. This is required for VSCode, for example,
    // which override `_load` to provide additional builtins to its extensions.

    if (parentApi === null)
      return originalModuleLoad(request, parent, isMain);

    // The 'pnpapi' name is reserved to return the PnP api currently in use
    // by the program

    if (request === `pnpapi`)
      return parentApi;

    // Request `Module._resolveFilename` (ie. `resolveRequest`) to tell us
    // which file we should load

    const modulePath = Module._resolveFilename(request, parent, isMain);

    // We check whether the module is owned by the dependency tree of the
    // module that required it. If it isn't, then we need to create a new
    // store and possibly load its sandboxed PnP runtime.

    const isOwnedByRuntime = parentApi !== null
      ? parentApi.findPackageLocator(modulePath) !== null
      : false;

    const moduleApiPath = isOwnedByRuntime
      ? parentApiPath
      : opts.manager.findApiPathFor(npath.dirname(modulePath));

    const entry = moduleApiPath !== null
      ? opts.manager.getApiEntry(moduleApiPath)
      : {instance: null, cache: defaultCache};

    // Check if the module has already been created for the given file

    const cacheEntry = entry.cache[modulePath];
    if (cacheEntry)
      return cacheEntry.exports;

    // Create a new module and store it into the cache

    // @ts-expect-error
    const module = new Module(modulePath, parent);
    // @ts-expect-error
    module.pnpApiPath = moduleApiPath;

    entry.cache[modulePath] = module;

    // The main module is exposed as global variable

    if (isMain) {
      process.mainModule = module;
      module.id = `.`;
    }

    // Try to load the module, and remove it from the cache if it fails

    let hasThrown = true;

    try {
    // @ts-expect-error
      module.load(modulePath);
      hasThrown = false;
    } finally {
      if (hasThrown) {
        delete Module._cache[modulePath];
      }
    }

    return module.exports;
  };

  type IssuerSpec = {
    apiPath: PortablePath | null;
    path: NativePath | null;
    module: NodeModule | null | undefined;
  };

  function getIssuerSpecsFromPaths(paths: Array<NativePath>): Array<IssuerSpec> {
    return paths.map(path => ({
      apiPath: opts.manager.findApiPathFor(path),
      path,
      module: null,
    }));
  }

  function getIssuerSpecsFromModule(module: NodeModule | null | undefined): Array<IssuerSpec> {
    const issuer = getIssuerModule(module);

    const issuerPath = issuer !== null
      ? npath.dirname(issuer.filename)
      : process.cwd();

    return [{
      apiPath: opts.manager.getApiPathFromParent(issuer),
      path: issuerPath,
      module,
    }];
  }

  function makeFakeParent(path: string) {
    const fakeParent = new Module(``);

    const fakeFilePath = npath.join(path, `[file]` as Filename);
    fakeParent.paths = Module._nodeModulePaths(fakeFilePath);

    return fakeParent;
  }

  // Splits a require request into its components, or return null if the request is a file path
  const pathRegExp = /^(?![a-zA-Z]:[\\/]|\\\\|\.{0,2}(?:\/|$))((?:@[^/]+\/)?[^/]+)\/*(.*|)$/;

  const originalModuleResolveFilename = Module._resolveFilename;

  Module._resolveFilename = function(request: string, parent: (NodeModule & {pnpApiPath?: PortablePath}) | null | undefined, isMain: boolean, options?: {[key: string]: any}) {
    if (builtinModules.has(request))
      return request;

    if (!enableNativeHooks)
      return originalModuleResolveFilename.call(Module, request, parent, isMain, options);

    if (options && options.plugnplay === false) {
      const {plugnplay, ...rest} = options;

      // Workaround a bug present in some version of Node (now fixed)
      // https://github.com/nodejs/node/pull/28078
      const forwardedOptions = Object.keys(rest).length > 0
        ? rest
        : undefined;

      try {
        enableNativeHooks = false;
        return originalModuleResolveFilename.call(Module, request, parent, isMain, forwardedOptions);
      } finally {
        enableNativeHooks = true;
      }
    }

    // We check that all the options present here are supported; better
    // to fail fast than to introduce subtle bugs in the runtime.

    if (options) {
      const optionNames = new Set(Object.keys(options));
      optionNames.delete(`paths`);
      optionNames.delete(`plugnplay`);

      if (optionNames.size > 0) {
        throw makeError(
          ErrorCode.UNSUPPORTED,
          `Some options passed to require() aren't supported by PnP yet (${Array.from(optionNames).join(`, `)})`
        );
      }
    }

    const issuerSpecs = options && options.paths
      ? getIssuerSpecsFromPaths(options.paths)
      : getIssuerSpecsFromModule(parent);

    if (request.match(pathRegExp) === null) {
      const parentDirectory = parent?.filename != null
        ? npath.dirname(parent.filename)
        : null;

      const absoluteRequest = npath.isAbsolute(request)
        ? request
        : parentDirectory !== null
          ? npath.resolve(parentDirectory, request)
          : null;

      if (absoluteRequest !== null) {
        const apiPath = parentDirectory === npath.dirname(absoluteRequest) && parent?.pnpApiPath
          ? parent.pnpApiPath
          : opts.manager.findApiPathFor(absoluteRequest);

        if (apiPath !== null) {
          issuerSpecs.unshift({
            apiPath,
            path: parentDirectory,
            module: null,
          });
        }
      }
    }

    let firstError;

    for (const {apiPath, path, module} of issuerSpecs) {
      let resolution;

      const issuerApi = apiPath !== null
        ? opts.manager.getApiEntry(apiPath, true).instance
        : null;

      try {
        if (issuerApi !== null) {
          resolution = issuerApi.resolveRequest(request, path !== null ? `${path}/` : null);
        } else {
          if (path === null)
            throw new Error(`Assertion failed: Expected the path to be set`);

          resolution = originalModuleResolveFilename.call(Module, request, module || makeFakeParent(path), isMain);
        }
      } catch (error) {
        firstError = firstError || error;
        continue;
      }

      if (resolution !== null) {
        return resolution;
      }
    }

    const requireStack = getRequireStack(parent);

    Object.defineProperty(firstError, `requireStack`, {
      configurable: true,
      writable: true,
      enumerable: false,
      value: requireStack,
    });

    if (requireStack.length > 0)
      firstError.message += `\nRequire stack:\n- ${requireStack.join(`\n- `)}`;

    throw firstError;
  };

  const originalFindPath = Module._findPath;

  Module._findPath = function(request: string, paths: Array<string> | null | undefined, isMain: boolean) {
    if (request === `pnpapi`)
      return false;

    // Node sometimes call this function with an absolute path and a `null` set
    // of paths. This would cause the resolution to fail. To avoid that, we
    // fallback on the regular resolution. We only do this when `isMain` is
    // true because the Node default resolution doesn't handle well in-zip
    // paths, even absolute, so we try to use it as little as possible.
    if (!enableNativeHooks || (isMain && npath.isAbsolute(request)))
      return originalFindPath.call(Module, request, paths, isMain);

    for (const path of paths || []) {
      let resolution: string | false;

      try {
        const pnpApiPath = opts.manager.findApiPathFor(path);
        if (pnpApiPath !== null) {
          const api = opts.manager.getApiEntry(pnpApiPath, true).instance;
          resolution = api.resolveRequest(request, path) || false;
        } else {
          resolution = originalFindPath.call(Module, request, [path], isMain);
        }
      } catch (error) {
        continue;
      }

      if (resolution) {
        return resolution;
      }
    }

    return false;
  };

  patchFs(fs, new PosixFS(opts.fakeFs));
}

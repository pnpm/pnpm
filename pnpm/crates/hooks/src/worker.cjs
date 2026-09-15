const readline = require('node:readline');
const { pathToFileURL } = require('node:url');
let mod = null;
let loadErr = null;
let nextCallbackId = 0;
const pendingCallbacks = new Map();
async function ensureLoaded() {
  if (mod !== null || loadErr !== null) return;
  try { mod = pnpmfileIsMjs ? await import(pathToFileURL(pnpmfilePath).href) : require(pnpmfilePath); } catch (err) { loadErr = err && err.stack ? err.stack : String(err); }
}
const rl = readline.createInterface({ input: process.stdin });
rl.on('line', (line) => {
  let req;
  try { req = JSON.parse(line); } catch { return; }
  if (Object.prototype.hasOwnProperty.call(req, 'callbackResponse')) {
    const pending = pendingCallbacks.get(req.callbackResponse);
    if (!pending) return;
    pendingCallbacks.delete(req.callbackResponse);
    if (req.err) {
      const error = Object.assign(new Error(req.err.message), req.err);
      if (req.err.status != null) error.response = { status: req.err.status };
      pending.reject(error);
    } else {
      const result = req.ok;
      if (result && result.filesMap && !(result.filesMap instanceof Map)) {
        result.filesMap = new Map(Object.entries(result.filesMap));
      }
      pending.resolve(result);
    }
    return;
  }
  handle(req);
});
async function handle(req) {
  const id = req.id;
  const send = (obj) => process.stdout.write(JSON.stringify(Object.assign({ id }, obj)) + '\n');
  await ensureLoaded();
  if (loadErr !== null) { send({ err: loadErr }); return; }
  if (req.query === 'hasHooks') { send({ ok: mod != null && mod.hooks != null }); return; }
  if (req.query === 'hasFilterLog') {
    send({ ok: mod != null && mod.hooks != null && typeof mod.hooks.filterLog === 'function' });
    return;
  }
  try {
    const fn = mod && mod.hooks && mod.hooks[req.hook];
    const context = { log: (m) => send({ log: String(m) }) };
    if (req.target === 'resolvers') {
      const resolvers = mod && mod.resolvers;
      send({ ok: (Array.isArray(resolvers) ? resolvers.map((resolver) => ({
        canResolve: typeof resolver.canResolve === 'function',
        resolve: typeof resolver.resolve === 'function',
        shouldRefreshResolution: typeof resolver.shouldRefreshResolution === 'function',
      })) : []) });
      return;
    }
    if (req.target === 'resolver') {
      const resolvers = mod && mod.resolvers;
      const resolver = Array.isArray(resolvers) ? resolvers[req.index] : null;
      if (!resolver || typeof resolver[req.method] !== 'function') {
        send({ ok: null });
        return;
      }
      const args = req.payload || [];
      const res = await resolver[req.method](...args);
      send({ ok: res === undefined ? null : res });
      return;
    }
    if (req.target === 'fetchers') {
      const fetchers = mod && mod.fetchers;
      send({ ok: (Array.isArray(fetchers) ? fetchers.map((f) => ({
        canFetch: f != null && typeof f.canFetch === 'function',
        fetch: f != null && typeof f.fetch === 'function',
      })) : []) });
      return;
    }
    if (req.target === 'finders') {
      const finders = mod && mod.finders;
      send({ ok: (finders != null && typeof finders === 'object')
        ? Object.keys(finders).filter((name) => typeof finders[name] === 'function')
        : [] });
      return;
    }
    if (req.target === 'finder') {
      const finders = mod && mod.finders;
      const finder = (finders != null && typeof finders === 'object') ? finders[req.name] : null;
      if (typeof finder !== 'function') {
        send({ ok: false });
        return;
      }
      const ctx = req.ctx || {};
      const res = await finder({
        alias: ctx.alias,
        name: ctx.name,
        version: ctx.version,
        readManifest: () => ctx.manifest,
      });
      send({ ok: res === undefined ? false : res });
      return;
    }
    if (req.target === 'fetcher') {
      const fetchers = mod && mod.fetchers;
      const fetcher = Array.isArray(fetchers) ? fetchers[req.index] : null;
      if (!fetcher || typeof fetcher[req.method] !== 'function') {
        send({ ok: null });
        return;
      }
      const args = req.payload || [];
      if (req.method === 'fetch' && req.callbacks) {
        const callNative = (method, resolution, options) => {
          const callbackId = nextCallbackId++;
          return new Promise((resolve, reject) => {
            pendingCallbacks.set(callbackId, { resolve, reject });
            send({ callback: { id: callbackId, method, resolution, options } });
          });
        };
        const cafs = Object.freeze({
          ...(await callNative('cafsInfo')),
          tempDir: () => callNative('tempDir'),
        });
        const callBuiltin = (method, providedCafs, resolution, options) => {
          if (providedCafs !== cafs) {
            return Promise.reject(new Error('Built-in fetcher received an invalid CAFS handle'));
          }
          return callNative(method, resolution, options);
        };
        args[0] = cafs;
        args[3] = Object.freeze({
          localTarball: (handle, resolution, options) =>
            callBuiltin('localTarball', handle, resolution, options),
          remoteTarball: (handle, resolution, options) =>
            callBuiltin('remoteTarball', handle, resolution, options),
        });
      }
      const res = await fetcher[req.method](...args);
      if (req.method === 'canFetch') {
        send({ ok: { value: res === undefined ? null : res, resolution: args[1] } });
      } else if (res && res.filesMap instanceof Map) {
        send({ ok: { ...res, filesMap: Object.fromEntries(res.filesMap) } });
      } else {
        send({ ok: res === undefined ? null : res });
      }
      return;
    }
    if (req.hook === 'readPackage') {
      const pkg = req.payload;
      if (typeof fn !== 'function') { send({ ok: pkg }); return; }
      pkg.dependencies = pkg.dependencies ?? {};
      pkg.devDependencies = pkg.devDependencies ?? {};
      pkg.optionalDependencies = pkg.optionalDependencies ?? {};
      pkg.peerDependencies = pkg.peerDependencies ?? {};
      const newPkg = await fn(pkg, context);
      if (!newPkg) {
        throw new Error("readPackage hook did not return a package manifest object. Hook imported via " + pnpmfilePath);
      }
      for (const dep of ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"]) {
        const v = newPkg[dep];
        if (v != null && (typeof v !== "object" || Array.isArray(v))) {
          throw new Error("readPackage hook returned package manifest object's property '" + dep + "' must be an object. Hook imported via " + pnpmfilePath);
        }
      }
      send({ ok: newPkg });
    } else if (req.hook === 'beforePacking') {
      if (typeof fn !== 'function') { send({ ok: req.payload }); return; }
      const newPkg = await fn(req.payload, req.dir, context);
      send({ ok: newPkg == null ? req.payload : newPkg });
    } else if (req.hook === 'filterLog') {
      const res = (typeof fn === 'function') ? await fn(req.payload, context) : true;
      send({ ok: res });
    } else {
      const res = (typeof fn === 'function') ? await fn(req.payload, context) : null;
      send({ ok: res === undefined ? null : res });
    }
  } catch (err) {
    send({ err: err && err.stack ? err.stack : String(err) });
  }
}

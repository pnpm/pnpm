const { pathToFileURL } = require('node:url');

function normalizeEsmModule(loaded) {
  const hasNamedExport = ['hooks', 'finders', 'resolvers', 'fetchers']
    .some((name) => Object.prototype.hasOwnProperty.call(loaded, name));
  if (hasNamedExport || !Object.prototype.hasOwnProperty.call(loaded, 'default')) {
    return loaded;
  }
  return loaded.default;
}

async function loadPnpmfile(pnpmfilePath) {
  // `require` preserves the complete CommonJS `module.exports`. Newer Node.js
  // versions can return an ESM namespace here; older versions and async ESM
  // fall back to `import()`. Both ESM paths use the same normalization.
  try {
    const loaded = require(pnpmfilePath);
    return Object.prototype.toString.call(loaded) === '[object Module]'
      ? normalizeEsmModule(loaded)
      : loaded;
  } catch (error) {
    if (error?.code !== 'ERR_REQUIRE_ESM' && error?.code !== 'ERR_REQUIRE_ASYNC_MODULE') {
      throw error;
    }
  }
  return normalizeEsmModule(await import(pathToFileURL(pnpmfilePath).href));
}

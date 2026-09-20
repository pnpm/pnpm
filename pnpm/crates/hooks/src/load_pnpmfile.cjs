const { pathToFileURL } = require('node:url');

async function loadPnpmfile(pnpmfilePath) {
  // `require` preserves the complete CommonJS `module.exports`. ESM-only
  // failures fall back to `import()`, which follows the extension and nearest
  // package scope and preserves the module namespace even when it is empty.
  try {
    return require(pnpmfilePath);
  } catch (error) {
    if (error?.code !== 'ERR_REQUIRE_ESM' && error?.code !== 'ERR_REQUIRE_ASYNC_MODULE') {
      throw error;
    }
  }
  return import(pathToFileURL(pnpmfilePath).href);
}

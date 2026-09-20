const { pathToFileURL } = require('node:url');

async function loadPnpmfile(pnpmfilePath) {
  const loaded = await import(pathToFileURL(pnpmfilePath).href);
  const hasNamedExport = ['hooks', 'finders', 'resolvers', 'fetchers']
    .some((name) => Object.prototype.hasOwnProperty.call(loaded, name));
  if (hasNamedExport) {
    return loaded;
  }
  return loaded.default;
}

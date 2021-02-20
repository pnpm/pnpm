import libUrl from 'url';

async function esmFileLoader(filePath) => {
  try {
    const result = await import(libUrl.pathToFileURL(filePath));
    return result.default ? result.default : result;
  }
  catch (err) {
    console.error(`Failed to load ESM configuration file ${filePath}`);
    throw err;
  }
};

async function requireOrImportPnpmfile (pnpmfilePath) {
  try {
    return require(pnpmfilePath);
  } catch (err) {
    if (err.code === 'ERR_REQUIRE_ESM') {
      return esmFileLoader(pnpmfilePath);
    }
    throw err;
   }
}

export default requireOrImportPnpmfile;

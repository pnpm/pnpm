const fs = require("fs");
let content = fs.readFileSync("fs/indexed-pkg-importer/src/cloneDir.ts", "utf8");

content = content.replace(
  "const copyFileAsync = promisify(gracefulFs.copyFile)",
  ""
);

content = content.replace(
  "import gracefulFs from '@pnpm/fs.graceful-fs'",
  ""
);

content = content.replaceAll(
  "copyFileAsync",
  "copyFile"
);

// also need to import copyFile from node:fs/promises
content = content.replace(
  "utimes,",
  "utimes,\n  copyFile,"
);

fs.writeFileSync("fs/indexed-pkg-importer/src/cloneDir.ts", content);

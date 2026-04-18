const fs = require("fs");
let content = fs.readFileSync("fs/indexed-pkg-importer/src/index.ts", "utf8");

content = content.replace(
  "function cloneDirPkg (",
  "async function cloneDirPkg ("
);

content = content.replace(
  "if (cloneDir(srcDirPath, to)) {",
  "if (await cloneDir(srcDirPath, to)) {"
);

content = content.replace(
  "return Promise.resolve(\"clone-dir\")",
  "return \"clone-dir\""
);

content = content.replace(
  "return Promise.resolve(undefined)",
  "return undefined"
);

content = content.replace(
  "return Promise.resolve('clone-dir')",
  "return 'clone-dir'"
);

fs.writeFileSync("fs/indexed-pkg-importer/src/index.ts", content);

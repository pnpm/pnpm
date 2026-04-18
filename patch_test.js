const fs = require("fs");
let content = fs.readFileSync("fs/indexed-pkg-importer/test/cloneDir.test.ts", "utf8");

content = content.replace(
  "const result = cloneDir(src, dest)",
  "const result = await cloneDir(src, dest)"
);
// It appears multiple times, so use global replace or replaceAll
content = content.replaceAll(
  "const result = cloneDir(src, dest)",
  "const result = await cloneDir(src, dest)"
);

fs.writeFileSync("fs/indexed-pkg-importer/test/cloneDir.test.ts", content);

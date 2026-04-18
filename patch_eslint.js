const fs = require("fs");
let content = fs.readFileSync("fs/indexed-pkg-importer/src/cloneDir.ts", "utf8");
content = "/* eslint-disable no-await-in-loop */\n" + content;
fs.writeFileSync("fs/indexed-pkg-importer/src/cloneDir.ts", content);

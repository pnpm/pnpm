const fs = require("fs");
let content = fs.readFileSync("cspell.json", "utf8");
content = content.replace(
  "\"words\": [",
  "\"words\": [\n    \"APFS\",\n    \"Btrfs\",\n    \"BTRFS\",\n    \"clonefile\",\n    \"FICLONE\",\n    \"realfile\",\n    \"statfs\","
);
fs.writeFileSync("cspell.json", content);

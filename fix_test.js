const fs = require('fs');
let content = fs.readFileSync('fs/indexed-pkg-importer/test/cloneDir.test.ts', 'utf8');

content = content.replace(/test\((.*?), \(\) => \{/g, 'test(, async () => {');
content = content.replace(/testOnLinuxOnly\((.*?), \(\) => \{/g, 'testOnLinuxOnly(, async () => {');
content = content.replace(/testOnMacOSOnly\((.*?), \(\) => \{/g, 'testOnMacOSOnly(, async () => {');

fs.writeFileSync('fs/indexed-pkg-importer/test/cloneDir.test.ts', content);

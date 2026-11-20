import fs from 'fs';
import path from 'path';

const affectedDirs = [
  'cli/commands',
  'installing/commands',
  'patching/commands',
  'releasing/commands',
  'workspace/projects-filter',
  'exec/commands',
  'deps/compliance/commands',
  'deps/inspection/commands'
];

for (const dir of affectedDirs) {
  const pkgPath = path.join(process.cwd(), dir, 'package.json');
  try {
    const raw = fs.readFileSync(pkgPath, 'utf8');
    // Using simple regex substitution if it literally appears twice
    let count = 0;
    const lines = raw.split('\n');
    const newLines = [];
    for (const line of lines) {
      if (line.includes('"@pnpm/workspace.projects-filter"')) {
        count++;
        if (count > 1) {
          // Skip the second duplicate
          console.log(`Removed duplicate from ${pkgPath}`);
          continue;
        }
      }
      newLines.push(line);
    }
    
    // Check if the previous line now ends with a comma that it shouldn't, or similar
    // Actually, just using regex might leave a trailing comma.
    
    // Alternative: since we know JS handles this mostly fine with JSON.parse:
    const asJson = JSON.parse(raw);
    const formatted = JSON.stringify(asJson, null, 2) + '\n';
    fs.writeFileSync(pkgPath, formatted, 'utf8');
    console.log(`Rewrote ${pkgPath}`);
  } catch(e) {
    console.log(`Failed on ${pkgPath}: ` + e);
  }
}

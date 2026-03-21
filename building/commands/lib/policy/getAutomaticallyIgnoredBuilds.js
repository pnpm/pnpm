import path from 'node:path';
import { parse } from '@pnpm/deps.path';
import { readModulesManifest } from '@pnpm/installing.modules-yaml';
export async function getAutomaticallyIgnoredBuilds(opts) {
    const modulesDir = getModulesDir(opts);
    const modulesManifest = await readModulesManifest(modulesDir);
    let automaticallyIgnoredBuilds;
    if (modulesManifest?.ignoredBuilds) {
        const ignoredPkgNames = new Set();
        for (const depPath of modulesManifest.ignoredBuilds) {
            ignoredPkgNames.add(parse(depPath).name ?? depPath);
        }
        automaticallyIgnoredBuilds = Array.from(ignoredPkgNames);
    }
    else {
        automaticallyIgnoredBuilds = null;
    }
    return {
        automaticallyIgnoredBuilds,
        modulesDir,
        modulesManifest,
    };
}
function getModulesDir(opts) {
    return opts.modulesDir ?? path.join(opts.lockfileDir ?? opts.dir, 'node_modules');
}
//# sourceMappingURL=getAutomaticallyIgnoredBuilds.js.map
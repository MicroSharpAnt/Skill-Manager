// Align the two runtime JS packages to Cargo's 2.11 / 2.7 minor versions.
// Uses only npm's existing cache. No network fallback and no source repo writes.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import {fileURLToPath} from 'node:url';
import {execFileSync} from 'node:child_process';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const pkg=JSON.parse(fs.readFileSync(path.join(root,'package.json'),'utf8'));
for(const name of ['@tauri-apps/api','@tauri-apps/plugin-dialog']){
 const version=pkg.dependencies[name],short=name.split('/')[1],target=path.join(root,'node_modules',name);
 try {if(JSON.parse(fs.readFileSync(path.join(target,'package.json'),'utf8')).version===version)continue;}catch{}
 const temp=fs.mkdtempSync(path.join(os.tmpdir(),'skill-manager-dependency-'));
 const url=`https://registry.npmjs.org/${name}/-/${short}-${version}.tgz`;
 const output=execFileSync('npm',['pack','--offline','--ignore-scripts',url,'--pack-destination',temp,'--json'],{encoding:'utf8'});
 const pack=JSON.parse(output)[0];
 if(pack.name!==name||pack.version!==version)throw new Error(`Unexpected package: ${pack.name}@${pack.version}`);
 const destination=path.join(root,'node_modules','.skill-manager-local',`${short}-${version}`);
 fs.mkdirSync(destination,{recursive:true});
 execFileSync('tar',['-xzf',path.join(temp,pack.filename),'-C',destination,'--strip-components=1']);
 const stat=fs.lstatSync(target,{throwIfNoEntry:false});
 if(stat?.isSymbolicLink())fs.unlinkSync(target);
 else if(stat)fs.renameSync(target,`${target}.backup-${Date.now()}`);
 fs.symlinkSync(path.relative(path.dirname(target),destination),target,'dir');
 console.log(`Aligned ${name}@${version} from offline cache`);
}

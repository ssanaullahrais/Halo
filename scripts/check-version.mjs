import fs from 'node:fs';
const read=p=>JSON.parse(fs.readFileSync(p,'utf8'));
const version=read('package.json').version;
const cargo=fs.readFileSync('src-tauri/Cargo.toml','utf8').match(/\[package\][\s\S]*?\nversion\s*=\s*"([^"]+)"/)?.[1];
for(const [name,value]of Object.entries({tauri:read('src-tauri/tauri.conf.json').version,cargo,lock:read('package-lock.json').packages[''].version}))if(value!==version)throw Error(`${name} version ${value} differs from ${version}`);
const tag=process.env.GITHUB_REF_TYPE==='tag'?process.env.GITHUB_REF_NAME:process.argv[2];
if(tag&&tag!==`v${version}`)throw Error(`Tag must be v${version}, received ${tag}`);
console.log(`Halo v${version}: versions match${tag?' the release tag':''}.`);

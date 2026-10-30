import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const config = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8'));
const frontend = JSON.parse(readFileSync('frontend/package.json', 'utf8'));
const lock = JSON.parse(readFileSync('frontend/package-lock.json', 'utf8'));
assert.equal(lock.version, frontend.version, 'Frontend lockfile version must match');
assert.equal(lock.packages[''].version, frontend.version, 'Frontend lockfile package version must match');
const flake = readFileSync('flake.nix', 'utf8');
assert.match(flake, /version = \(builtins\.fromTOML \(builtins\.readFile \.\/Cargo\.toml\)\)\.workspace\.package\.version;/, 'Nix must derive its version from Cargo');
const cargo = readFileSync('Cargo.toml', 'utf8');
const workspace = cargo.split('[workspace.package]')[1]?.split('\n[')[0];
const version = workspace?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
assert.equal(config.version, version, 'Tauri and Cargo versions must match');
assert.equal(frontend.version, version, 'Frontend and Cargo versions must match');
const cargoPackages = readFileSync('Cargo.lock', 'utf8').split('[[package]]');
for (const name of ['autofill-core', 'autofill-automation', 'mpc-autofill']) {
  const entry = cargoPackages.find((block) => block.match(/^name\s*=\s*"([^"]+)"/m)?.[1] === name);
  assert.equal(entry?.match(/^version\s*=\s*"([^"]+)"/m)?.[1], version, `${name} lockfile version must match`);
}
const license = workspace?.match(/^license\s*=\s*"([^"]+)"/m)?.[1];
assert.equal(license, 'GPL-3.0-only', 'Workspace license must preserve the upstream GPLv3 terms');
assert.equal(config.bundle.license, license, 'Installer and Cargo licenses must match');
const licenseText = readFileSync('LICENSE', 'utf8');
assert.match(licenseText, /GNU GENERAL PUBLIC LICENSE\s+Version 3, 29 June 2007/);
assert.ok(readFileSync('NOTICE.md', 'utf8').includes('https://github.com/chilli-axe/mpc-autofill'));
const rustLock = readFileSync('Cargo.lock', 'utf8').replace(/\r\n/g, '\n');
for (const [rust, npm] of [['tauri', '@tauri-apps/api'], ['tauri-plugin-dialog', '@tauri-apps/plugin-dialog']]) {
  const entry = rustLock.split('[[package]]').find(block => block.includes(`\nname = "${rust}"\n`));
  const rustVersion = entry?.match(/^version = "([^"]+)"/m)?.[1];
  const jsVersion = lock.packages[`node_modules/${npm}`]?.version;
  assert.ok(rustVersion && jsVersion, `${rust} and ${npm} must be locked`);
  assert.equal(jsVersion.split('.').slice(0, 2).join('.'), rustVersion.split('.').slice(0, 2).join('.'), `${rust} and ${npm} must use the same major/minor versions`);
}
if (process.env.RELEASE_TAG) {
  assert.equal(process.env.RELEASE_TAG, `v${version}`, 'Tag must match the app version');
}
console.log(`Release version: ${version}`);

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const config = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8'));
const frontend = JSON.parse(readFileSync('frontend/package.json', 'utf8'));
const cargo = readFileSync('Cargo.toml', 'utf8');
const workspace = cargo.split('[workspace.package]')[1]?.split('\n[')[0];
const version = workspace?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
assert.equal(config.version, version, 'Tauri and Cargo versions must match');
assert.equal(frontend.version, version, 'Frontend and Cargo versions must match');
const license = workspace?.match(/^license\s*=\s*"([^"]+)"/m)?.[1];
assert.equal(license, 'GPL-3.0-only', 'Workspace license must preserve the upstream GPLv3 terms');
assert.equal(config.bundle.license, license, 'Installer and Cargo licenses must match');
const licenseText = readFileSync('LICENSE', 'utf8');
assert.match(licenseText, /GNU GENERAL PUBLIC LICENSE\s+Version 3, 29 June 2007/);
assert.ok(readFileSync('NOTICE.md', 'utf8').includes('https://github.com/chilli-axe/mpc-autofill'));
if (process.env.RELEASE_TAG) {
  assert.equal(process.env.RELEASE_TAG, `v${version}`, 'Tag must match the app version');
}
console.log(`Release version: ${version}`);

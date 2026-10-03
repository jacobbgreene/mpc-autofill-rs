import assert from 'node:assert/strict';
import { copyFileSync, mkdirSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

const bundles = process.platform === 'win32'
  ? [['nsis', '.exe']]
  : [['appimage', '.AppImage'], ['deb', '.deb']];
mkdirSync('dist', { recursive: true });
for (const [bundle, extension] of bundles) {
  const directory = join('target', 'release', 'bundle', bundle);
  const files = readdirSync(directory).filter(name => name.endsWith(extension));
  assert.equal(files.length, 1, `Expected exactly one ${bundle} installer`);
  copyFileSync(join(directory, files[0]), join('dist', files[0]));
  console.log(`Installer: ${files[0]}`);
}

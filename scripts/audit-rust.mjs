// cargo-audit skips path packages. Audit a disposable registry-shaped copy too,
// so carrying one verified GLib backport cannot hide later upstream advisories.
import './check-glib-backport.mjs';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
const binary = process.env.MPC_CARGO_AUDIT || 'cargo-audit';
const version = spawnSync(binary, ['--version'], { encoding: 'utf8' });
assert.ifError(version.error);
assert.equal(version.status, 0, version.stderr);
assert.match(version.stdout, /cargo-audit 0\.22\.2\b/, 'Use the pinned cargo-audit 0.22.2');
const reportDirectory = join(root, 'target', 'security-review');
mkdirSync(reportDirectory, { recursive: true });
const database = join(reportDirectory, 'advisory-db');
const tempRoot = resolve(tmpdir());
const temporary = mkdtempSync(join(tempRoot, 'mpc-vendor-audit-'));
const advisory = 'RUSTSEC-2024-0429';
const provenance = JSON.parse(readFileSync(join(root, 'vendor', 'glib', 'UPSTREAM-INTEGRITY.json'), 'utf8'));
const lock = readFileSync(join(root, 'Cargo.lock'), 'utf8');
const sections = lock.split('[[package]]');
const index = sections.findIndex((section) => /^name = "glib"$/m.test(section));
assert.ok(index > 0);
sections[index] = sections[index].replace(/^version = "0\.18\.5"$/m,
  `version = "0.18.5"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "${provenance.archive_sha256}"`);
const shadow = join(temporary, 'Cargo.lock');
writeFileSync(shadow, sections.join('[[package]]'));

let failed = false;
try {
  for (const [label, file, ignored] of [
    ['actual', join(root, 'Cargo.lock'), []],
    ['vendor-upstream', shadow, [advisory]],
  ]) {
    const args = ['audit', '--file', file, '--db', database, '--deny', 'unsound', '--json'];
    if (label !== 'actual') args.push('--no-fetch');
    for (const id of ignored) args.push('--ignore', id);
    const result = spawnSync(binary, args, { cwd: root, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 });
    assert.ifError(result.error);
    if (result.stderr) process.stderr.write(result.stderr);
    let report;
    try { report = JSON.parse(result.stdout); }
    catch { throw new Error(`cargo-audit failed to produce a report (${label}, exit ${result.status})`); }
    // Local user configuration must not silently weaken this security gate.
    assert.deepEqual(report.settings.ignore, ignored, 'Unexpected advisory exclusions');
    assert.deepEqual(report.settings.target_arch, [], 'Audit must include every target architecture');
    assert.deepEqual(report.settings.target_os, [], 'Audit must include Windows and Linux dependencies');
    assert.equal(report.settings.severity ?? null, null, 'Do not filter lower-severity vulnerabilities');
    assert.ok(report.settings.informational_warnings.includes('unsound'), 'Unsoundness advisories must be checked');
    assert.ok(Array.isArray(report.vulnerabilities.list));
    writeFileSync(join(reportDirectory, `cargo-audit-${label}.json`), JSON.stringify(report, null, 2) + '\n');
    const warnings = Object.entries(report.warnings).flatMap(([kind, entries]) => entries.map((entry) => ({ kind, ...entry })));
    console.log(`${label}: ${report.vulnerabilities.list.length} vulnerabilities; ${warnings.length} warnings`);
    for (const entry of [...report.vulnerabilities.list, ...warnings]) {
      console.log(`  ${entry.kind || 'vulnerability'}: ${entry.advisory?.id || 'unidentified'} ${entry.package?.name || ''} ${entry.package?.version || ''}`);
    }
    if (result.status !== 0 || report.vulnerabilities.list.length > 0 || warnings.some((entry) => entry.kind === 'unsound')) failed = true;
  }
} finally {
  // Only the exact directory created above is removable, never a caller path.
  assert.equal(dirname(resolve(temporary)), tempRoot);
  assert.ok(basename(temporary).startsWith('mpc-vendor-audit-'));
  rmSync(temporary, { recursive: true, force: true });
}
assert.equal(failed, false, 'Rust dependency audit failed; review the saved reports');
console.log('Rust audit passed, including future-advisory coverage for the verified GLib backport.');

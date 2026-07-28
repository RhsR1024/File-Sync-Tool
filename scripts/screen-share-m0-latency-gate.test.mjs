import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { evaluateM0Manifest, runCli } from './screen-share-m0-latency-gate.mjs';

const thresholds = Object.fromEntries(['capture_to_display_ms', 'input_to_sendinput_ms', 'input_to_visible_response_ms'].map((name) => [name, { p50: 50, p95: 100, p99: 150 }]));
const metrics = Object.fromEntries(Object.keys(thresholds).map((name) => [name, { p50: 20, p95: 40, p99: 60 }]));
function fixture({ latency = { scope: 'm0-latency-samples', status: 'passed', metrics }, causal = { scope: 'input-to-visible-causal-evidence', status: 'passed', method: 'pixel', causal_link: true } } = {}) {
  const dir = mkdtempSync(join(tmpdir(), 'm0-gate-')); writeFileSync(join(dir, 'latency.json'), JSON.stringify(latency)); writeFileSync(join(dir, 'causal.json'), JSON.stringify(causal));
  return { dir, manifest: { schema_version: 1, latency: { report: 'latency.json', thresholds_ms: thresholds }, input_to_visible: { report: 'causal.json' } } };
}
test('complete latency and controlled causal evidence pass', () => { const { dir, manifest } = fixture(); try { const result = evaluateM0Manifest(manifest, { manifestDirectory: dir }); assert.equal(result.status, 'passed'); assert.equal(result.recommended_exit_code, 0); } finally { rmSync(dir, { recursive: true, force: true }); } });
test('latency threshold failure is explicit', () => { const bad = { scope: 'm0-latency-samples', status: 'passed', metrics: { ...metrics, capture_to_display_ms: { p50: 20, p95: 40, p99: 200 } } }; const { dir, manifest } = fixture({ latency: bad }); try { assert.equal(evaluateM0Manifest(manifest, { manifestDirectory: dir }).status, 'failed'); } finally { rmSync(dir, { recursive: true, force: true }); } });
test('latency pass without causal evidence remains incomplete', () => { const { dir, manifest } = fixture({ causal: { scope: 'input-to-visible-causal-evidence', status: 'passed', method: 'proxy', causal_link: false } }); try { const result = evaluateM0Manifest(manifest, { manifestDirectory: dir }); assert.equal(result.status, 'incomplete'); assert.equal(result.recommended_exit_code, 2); } finally { rmSync(dir, { recursive: true, force: true }); } });
test('invalid paths and schema return invalid', () => { assert.equal(evaluateM0Manifest({ schema_version: 1 }).status, 'invalid'); const { dir, manifest } = fixture(); manifest.latency.report = 'missing.json'; try { assert.equal(evaluateM0Manifest(manifest, { manifestDirectory: dir }).status, 'invalid'); } finally { rmSync(dir, { recursive: true, force: true }); } });
test('collect-only preserves JSON recommendation and writes markdown', () => { const { dir, manifest } = fixture({ causal: { scope: 'input-to-visible-causal-evidence', status: 'passed', method: 'proxy', causal_link: false } }); const manifestPath = join(dir, 'manifest.json'); const output = join(dir, 'm0.json'); const markdown = join(dir, 'm0.md'); writeFileSync(manifestPath, JSON.stringify(manifest)); try { assert.equal(runCli(['--manifest', manifestPath, '--output', output, '--markdown', markdown, '--collect-only']), 0); assert.equal(JSON.parse(readFileSync(output, 'utf8')).recommended_exit_code, 2); assert.match(readFileSync(markdown, 'utf8'), /Status: `incomplete`/); } finally { rmSync(dir, { recursive: true, force: true }); } });

import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { evaluateWgcStabilityManifest, runCli } from './screen-share-wgc-stability-gate.mjs';

const thresholds = { minimum_capture_duration_minutes: 30, maximum_resource_growth_mb: 100, maximum_handle_growth: 20, minimum_monitors: 2, maximum_black_frame_ratio: 0.01, maximum_consecutive_black_frames: 2, maximum_frame_gap_ms: 500, minimum_frames_after_recovery: 30 };
const evidence = {
  scope: 'wgc-stability-recovery-evidence', status: 'passed',
  capture_stability: { completed: true, duration_minutes: 30, unexpected_capture_failures: 0 },
  resource_growth: { unacceptable_leak: false, process_growth_mb: 15, handle_growth: 2 },
  recovery: { lock_screen: { tested: true, recovered: true }, display_reconfiguration: { tested: true, recovered: true } },
  multi_monitor: { tested: true, covered_monitor_count: 2 },
  black_frame_detection: { total_frames: 1000, detected_black_frames: 1, max_consecutive_black_frames: 1 },
  recovery_event_accounting: { lock_screen: { observed_count: 1, recovered_count: 1, failed_count: 0 }, display_reconfiguration: { observed_count: 1, recovered_count: 1, failed_count: 0 } },
  frame_continuity: { continuous: true, max_gap_ms: 100, frames_after_recovery: 60 },
  external_artifacts: [{ id: 'capture-trace', reference: 'external://capture-trace.json', sha256: 'a'.repeat(64) }],
};
function fixture(report = evidence) { const dir = mkdtempSync(join(tmpdir(), 'wgc-gate-')); writeFileSync(join(dir, 'evidence.json'), JSON.stringify(report)); return { dir, manifest: { schema_version: 1, evidence: { report: 'evidence.json' }, thresholds } }; }
test('complete attributed WGC evidence passes', () => { const { dir, manifest } = fixture(); try { const result = evaluateWgcStabilityManifest(manifest, { manifestDirectory: dir }); assert.equal(result.status, 'passed'); assert.equal(result.recommended_exit_code, 0); } finally { rmSync(dir, { recursive: true, force: true }); } });
test('resource or black-frame threshold violations fail explicitly', () => { const report = { ...evidence, resource_growth: { ...evidence.resource_growth, process_growth_mb: 101 }, black_frame_detection: { ...evidence.black_frame_detection, detected_black_frames: 20 } }; const { dir, manifest } = fixture(report); try { const result = evaluateWgcStabilityManifest(manifest, { manifestDirectory: dir }); assert.equal(result.status, 'failed'); assert.equal(result.recommended_exit_code, 1); } finally { rmSync(dir, { recursive: true, force: true }); } });
test('explicit leak fails but a missing leak declaration remains incomplete', () => { const { dir, manifest } = fixture({ ...evidence, resource_growth: { ...evidence.resource_growth, unacceptable_leak: true } }); try { assert.equal(evaluateWgcStabilityManifest(manifest, { manifestDirectory: dir }).status, 'failed'); } finally { rmSync(dir, { recursive: true, force: true }); } const second = fixture({ ...evidence, resource_growth: { process_growth_mb: 1, handle_growth: 1 } }); try { assert.equal(evaluateWgcStabilityManifest(second.manifest, { manifestDirectory: second.dir }).status, 'incomplete'); } finally { rmSync(second.dir, { recursive: true, force: true }); } });
test('missing lock-screen recovery and external artifacts remain incomplete', () => { const report = { ...evidence, recovery: { ...evidence.recovery, lock_screen: { tested: false, recovered: false } }, external_artifacts: [] }; const { dir, manifest } = fixture(report); try { const result = evaluateWgcStabilityManifest(manifest, { manifestDirectory: dir }); assert.equal(result.status, 'incomplete'); assert.equal(result.recommended_exit_code, 2); } finally { rmSync(dir, { recursive: true, force: true }); } });
test('invalid manifest or evidence path is invalid', () => { assert.equal(evaluateWgcStabilityManifest({ schema_version: 1 }).status, 'invalid'); const { dir, manifest } = fixture(); manifest.evidence.report = 'missing.json'; try { assert.equal(evaluateWgcStabilityManifest(manifest, { manifestDirectory: dir }).status, 'invalid'); } finally { rmSync(dir, { recursive: true, force: true }); } });
test('collect-only preserves recommendation and writes markdown', () => { const report = { ...evidence, multi_monitor: { tested: true, covered_monitor_count: 1 } }; const { dir, manifest } = fixture(report); const manifestPath = join(dir, 'manifest.json'); const output = join(dir, 'wgc.json'); const markdown = join(dir, 'wgc.md'); writeFileSync(manifestPath, JSON.stringify(manifest)); try { assert.equal(runCli(['--manifest', manifestPath, '--output', output, '--markdown', markdown, '--collect-only']), 0); assert.equal(JSON.parse(readFileSync(output, 'utf8')).recommended_exit_code, 2); assert.match(readFileSync(markdown, 'utf8'), /Status: `incomplete`/); } finally { rmSync(dir, { recursive: true, force: true }); } });

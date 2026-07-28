import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import test from 'node:test';
import { REQUIRED_ROLES, evaluateManifest } from './screen-share-qualification-aggregate.mjs';

function candidate(gpu) {
  return { name: gpu ? 'GPU encoder' : 'System encoder', admitted: true, hardware: true, gpu_surface_input: gpu, activation_succeeded: true, configuration_succeeded: true, input_adapter: gpu ? { description: 'Test GPU', vendor_id: '10de', device_id: '1f82', luid: '0x00000000:00000001', driver_version: null, pnp_device_id: null } : null, activation_adapter_luid: gpu ? '0x00000000:00000001' : null, luid_match: gpu ? true : null, gpu_surface_pool_recycled: null, self_test: { attempted: true, passed: true, produced_access_units: 3, found_sps: true, found_pps: true, found_idr: true, timeline_monotonic: true, timestamps_from_encoder: true, durations_from_encoder: true, dynamic_pattern_input: true, decoder_frame_count: 3, gpu_surface_input: gpu, b_slice_count: 0, baseline_profile_confirmed: true } };
}
function report({ gpu = true, failed = false, skipped = false, v3 = false } = {}) {
  const status = failed ? 'failed' : skipped ? 'skipped' : 'passed';
  const value = { schema_version: v3 ? 3 : 2, qualification_status: failed ? 'failed' : skipped ? 'incomplete' : 'passed', steps: [
    { name: 'environment_inventory', status: 'passed', exit_code: 0, timed_out: false, log_path: 'environment.log' },
    { name: 'screen_share_web_build', status: 'passed', exit_code: 0, timed_out: false, log_path: 'build.log' },
    { name: 'mf_system_memory_self_test', status: 'passed', exit_code: 0, timed_out: false, log_path: 'mf.log' },
    { name: 'gpu_dxgi_surface_self_test', status, exit_code: failed ? 1 : skipped ? null : 0, timed_out: false, log_path: 'gpu.log' },
    { name: 'browser_capability_probe', status: 'passed', exit_code: 0, timed_out: false, log_path: 'browser.log' },
  ], browser_report: 'browser.json', media_foundation: { system_memory_candidate_reports: [candidate(false)], gpu_dxgi_candidate_reports: gpu ? [candidate(true)] : [candidate(false)] } };
  if (v3) {
    value.source = { git_commit: 'test-commit', dirty: false, app_version: '1.2.0' };
    value.host = { hostname: 'test-host', input_adapter: { vendor_id: '10de', device_id: '1f82', luid: '0x00000000:00000001' } };
    value.media_foundation.structured_evidence = {
      system_memory: { candidate_total: 1, candidate_parsed: 1, candidate_malformed: 0, candidates: [candidate(false)], gate_assertions: { input_adapter_identity: true, activation_adapter_luid: '0x00000000:00000001', luid_match: true, pool_recycled: null } },
      gpu_dxgi_surface: { candidate_total: 1, candidate_parsed: 1, candidate_malformed: 0, candidates: gpu ? [candidate(true)] : [candidate(false)], gate_assertions: { input_adapter_identity: true, activation_adapter_luid: '0x00000000:00000001', luid_match: true, pool_recycled: true } },
    };
  }
  return value;
}
function hash(path) { return createHash('sha256').update(readFileSync(path)).digest('hex'); }
function fixture(runs) {
  const dir = mkdtempSync(join(tmpdir(), 'qualification-aggregate-'));
  for (const [name, value] of Object.entries(runs)) {
    for (const file of ['environment.log', 'build.log', 'mf.log', 'gpu.log', 'browser.log']) writeFileSync(join(dir, file), 'evidence');
    writeFileSync(join(dir, 'browser.json'), JSON.stringify({ browsers: [{ result: { rtcPeerConnectionConstructed: true } }] }));
    if (value.schema_version >= 3 && !value.artifacts) {
      value.artifacts = {
        environment_report: { relative_path: 'environment.log', sha256: hash(join(dir, 'environment.log')) },
        browser_report: { relative_path: 'browser.json', sha256: hash(join(dir, 'browser.json')) },
        step_logs: Object.fromEntries(['environment_inventory', 'screen_share_web_build', 'mf_system_memory_self_test', 'gpu_dxgi_surface_self_test', 'browser_capability_probe'].map((step, index) => {
          const file = ['environment.log', 'build.log', 'mf.log', 'gpu.log', 'browser.log'][index]; return [step, { relative_path: file, sha256: hash(join(dir, file)) }];
        })),
      };
    }
    writeFileSync(join(dir, `${name}.json`), JSON.stringify(value));
  }
  return { dir, manifest: { schema_version: 1, runs: Object.keys(runs).map((id) => ({ id, report: `${id}.json`, required: true, roles: [id] })) } };
}
test('all required target roles with strict startup evidence pass', () => {
  const runs = Object.fromEntries(REQUIRED_ROLES.map((role) => [role, report({ v3: true })])); const { dir, manifest } = fixture(runs);
  try { const result = evaluateManifest(manifest, { manifestDirectory: dir }); assert.equal(result.startup_matrix_status, 'passed'); assert.equal(result.exit_code_recommendation, 0); } finally { rmSync(dir, { recursive: true, force: true }); }
});
test('required explicit failure is retained', () => {
  const { dir, manifest } = fixture(Object.fromEntries(REQUIRED_ROLES.map((role) => [role, report({ failed: role === 'm3:nvidia' })])));
  try { const result = evaluateManifest(manifest, { manifestDirectory: dir }); assert.equal(result.startup_matrix_status, 'failed'); assert.equal(result.exit_code_recommendation, 1); assert.ok(result.observed_failures.some((failure) => failure.run_id === 'm3:nvidia')); } finally { rmSync(dir, { recursive: true, force: true }); }
});
test('missing role and skipped evidence are incomplete', () => {
  const { dir, manifest } = fixture({ 'm2:intel-broadwell': report({ skipped: true }) });
  try { const result = evaluateManifest(manifest, { manifestDirectory: dir }); assert.equal(result.startup_matrix_status, 'incomplete'); assert.equal(result.exit_code_recommendation, 2); } finally { rmSync(dir, { recursive: true, force: true }); }
});
test('duplicate run and duplicate step names are invalid', () => {
  const duplicateRuns = { schema_version: 1, runs: [{ id: 'x', report: 'x.json', roles: [] }, { id: 'x', report: 'x.json', roles: [] }] };
  assert.equal(evaluateManifest(duplicateRuns).exit_code_recommendation, 3);
  const { dir, manifest } = fixture({ 'm2:intel-broadwell': { ...report(), steps: [{ name: 'mf_system_memory_self_test', status: 'passed', exit_code: 0, timed_out: false }, { name: 'mf_system_memory_self_test', status: 'passed', exit_code: 0, timed_out: false }] } });
  try { assert.equal(evaluateManifest(manifest, { manifestDirectory: dir }).exit_code_recommendation, 3); } finally { rmSync(dir, { recursive: true, force: true }); }
});
test('CPU fallback candidate cannot satisfy an M3 GPU role', () => {
  const { dir, manifest } = fixture(Object.fromEntries(REQUIRED_ROLES.map((role) => [role, report({ gpu: role !== 'm3:nvidia' })])));
  try { const result = evaluateManifest(manifest, { manifestDirectory: dir }); assert.notEqual(result.coverage['m3:nvidia'].status, 'passed'); assert.equal(result.exit_code_recommendation, 2); } finally { rmSync(dir, { recursive: true, force: true }); }
});
test('collect-only is an execution option and does not mutate evaluated matrix state', () => {
  const { dir, manifest } = fixture({ 'm2:intel-broadwell': report({ skipped: true }) });
  try { const strict = evaluateManifest(manifest, { manifestDirectory: dir }); const collect = evaluateManifest(manifest, { manifestDirectory: dir, collectOnly: true }); assert.equal(strict.startup_matrix_status, collect.startup_matrix_status); assert.equal(strict.exit_code_recommendation, collect.exit_code_recommendation); } finally { rmSync(dir, { recursive: true, force: true }); }
});
test('source qualification failure cannot be upgraded by otherwise passing gates', () => {
  const runs = Object.fromEntries(REQUIRED_ROLES.map((role) => [role, { ...report(), qualification_status: role === 'm2:intel-10th' ? 'failed' : 'passed' }])); const { dir, manifest } = fixture(runs);
  try { const result = evaluateManifest(manifest, { manifestDirectory: dir }); assert.equal(result.startup_matrix_status, 'failed'); assert.equal(result.exit_code_recommendation, 1); } finally { rmSync(dir, { recursive: true, force: true }); }
});
test('CLI writes an aggregate artifact and collect-only only changes process exit behavior', () => {
  const { dir, manifest } = fixture({ 'm2:intel-broadwell': report({ skipped: true }) });
  const manifestPath = join(dir, 'manifest.json'); const outputPath = join(dir, 'result.json'); writeFileSync(manifestPath, JSON.stringify(manifest));
  try {
    execFileSync(process.execPath, ['scripts/screen-share-qualification-aggregate.mjs', '--manifest', manifestPath, '--output', outputPath, '--collect-only'], { cwd: process.cwd() });
    const result = JSON.parse(readFileSync(outputPath, 'utf8'));
    assert.equal(result.startup_matrix_status, 'incomplete');
    assert.equal(result.exit_code_recommendation, 2);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
test('missing required step remains incomplete', () => {
  const incomplete = report(); incomplete.steps = incomplete.steps.filter((step) => step.name !== 'screen_share_web_build');
  const { dir, manifest } = fixture({ 'm2:intel-broadwell': incomplete });
  try { assert.equal(evaluateManifest(manifest, { manifestDirectory: dir }).exit_code_recommendation, 2); } finally { rmSync(dir, { recursive: true, force: true }); }
});
test('v3 evidence gaps, missing hashes, and malformed candidates cannot pass', () => {
  const missingAdapter = report({ v3: true }); delete missingAdapter.host.input_adapter.luid;
  const missingHash = report({ v3: true }); missingHash.artifacts = { environment_report: { relative_path: 'environment.log' } };
  const malformed = report({ v3: true }); malformed.media_foundation.structured_evidence.gpu_dxgi_surface.candidate_malformed = 1;
  const { dir, manifest } = fixture({ 'm2:intel-broadwell': missingAdapter, 'm2:intel-skylake': missingHash, 'm2:intel-10th': malformed });
  try {
    const result = evaluateManifest(manifest, { manifestDirectory: dir });
    assert.equal(result.runs[0].status, 'incomplete');
    assert.equal(result.runs[1].status, 'incomplete');
    assert.equal(result.runs[2].status, 'incomplete');
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
test('v3 LUID mismatch and missing pool recycle assertion cannot satisfy a GPU role', () => {
  const mismatch = report({ v3: true }); mismatch.media_foundation.structured_evidence.gpu_dxgi_surface.gate_assertions.luid_match = false;
  const noPool = report({ v3: true }); noPool.media_foundation.structured_evidence.gpu_dxgi_surface.gate_assertions.pool_recycled = null;
  const { dir, manifest } = fixture({ 'm3:nvidia': mismatch, 'm3:amd': noPool });
  try {
    const result = evaluateManifest(manifest, { manifestDirectory: dir });
    assert.equal(result.coverage['m3:nvidia'].status, 'incomplete');
    assert.equal(result.coverage['m3:amd'].status, 'incomplete');
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

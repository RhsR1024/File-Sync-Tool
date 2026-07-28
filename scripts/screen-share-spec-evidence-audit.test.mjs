import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { REQUIRED_GATES, evaluateSpecEvidenceManifest, renderMarkdown, runCli } from './screen-share-spec-evidence-audit.mjs';
import { FIELD_EVIDENCE_FIXTURES } from './screen-share-field-evidence.fixtures.mjs';

function fixture(reports, gates = Object.keys(reports)) {
  const dir = mkdtempSync(join(tmpdir(), 'spec-evidence-audit-'));
  for (const [id, report] of Object.entries(reports)) writeFileSync(join(dir, `${id}.json`), JSON.stringify(report));
  return { dir, manifest: { schema_version: 1, gates: gates.map((id) => ({ id, report: `${id}.json` })) } };
}
/** The three machine-generated gates are consumed as their tool's own output. */
function reportFor(id, status = 'passed') {
  const definition = REQUIRED_GATES[id];
  if (FIELD_EVIDENCE_FIXTURES[id]) return FIELD_EVIDENCE_FIXTURES[id]({ status });
  const report = { scope: definition.scope, status };
  if (id === 'startup_matrix') { delete report.status; report.startup_matrix_status = status; report.spec_completion = 'not_evaluated'; }
  return report;
}
function allReports(status = 'passed') {
  return Object.fromEntries(Object.keys(REQUIRED_GATES).map((id) => [id, reportFor(id, status)]));
}
test('all missing evidence is incomplete and never full spec pass', () => {
  const result = evaluateSpecEvidenceManifest({ schema_version: 1, gates: [] });
  assert.equal(result.audit_status, 'incomplete'); assert.equal(result.spec_completion, 'not_evaluated'); assert.equal(result.exit_code_recommendation, 2);
});
test('M0 gate requires the fixed underscore scope', () => {
  assert.equal(REQUIRED_GATES.m0_latency_input_visible.scope, 'm0_latency_input_visible');
});
test('WGC gate requires the fixed underscore scope', () => {
  assert.equal(REQUIRED_GATES.wgc_stability_recovery.scope, 'wgc_stability_recovery');
});
test('startup qualification and browser loopback cannot satisfy full spec evidence', () => {
  const reports = {
    startup_matrix: reportFor('startup_matrix'),
    managed_browser_external_media: { scope: 'browser-capability-probe', status: 'passed', managed_browser_external_acceptance: false, webrtc_loopback_media: { success: true } },
  }; const { dir, manifest } = fixture(reports);
  try { const result = evaluateSpecEvidenceManifest(manifest, { manifestDirectory: dir }); assert.equal(result.audit_status, 'incomplete'); assert.equal(result.gates.managed_browser_external_media.status, 'incomplete'); } finally { rmSync(dir, { recursive: true, force: true }); }
});
test('a declared status without structured field evidence never closes a gate', () => {
  const reports = Object.fromEntries(Object.keys(REQUIRED_GATES).map((id) => {
    const definition = REQUIRED_GATES[id];
    if (!definition.fieldEvidence) return [id, reportFor(id)];
    return [id, { scope: definition.scope, status: 'passed', managed_browser_external_acceptance: true }];
  }));
  const { dir, manifest } = fixture(reports);
  try {
    const result = evaluateSpecEvidenceManifest(manifest, { manifestDirectory: dir });
    assert.equal(result.audit_status, 'incomplete');
    assert.equal(result.spec_completion, 'not_evaluated');
    for (const id of Object.keys(REQUIRED_GATES)) {
      if (!REQUIRED_GATES[id].fieldEvidence) continue;
      assert.equal(result.gates[id].status, 'incomplete', `${id} accepted an unstructured report`);
      assert.ok(result.gates[id].issues.length > 1, `${id} produced no structural findings`);
    }
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
test('complete synthetic manifest passes only when every fixed gate scope passes', () => {
  const { dir, manifest } = fixture(allReports());
  try { const result = evaluateSpecEvidenceManifest(manifest, { manifestDirectory: dir }); assert.equal(result.audit_status, 'passed'); assert.equal(result.spec_completion, 'passed'); assert.equal(result.exit_code_recommendation, 0); } finally { rmSync(dir, { recursive: true, force: true }); }
});
test('manifest thresholds tighten a field gate that its own report declared passing', () => {
  const { dir, manifest } = fixture(allReports());
  const impairment = manifest.gates.find((gate) => gate.id === 'network_impairment_recovery');
  impairment.thresholds = { maximum_recovery_p99_ms: 400, maximum_frame_gap_ms: 500 };
  try {
    const result = evaluateSpecEvidenceManifest(manifest, { manifestDirectory: dir });
    assert.equal(result.gates.network_impairment_recovery.status, 'failed');
    assert.equal(result.exit_code_recommendation, 1);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
test('failed report and invalid duplicate or missing references retain distinct exits', () => {
  const reports = allReports();
  reports.network_impairment_recovery = reportFor('network_impairment_recovery', 'failed');
  const { dir, manifest } = fixture(reports);
  try { assert.equal(evaluateSpecEvidenceManifest(manifest, { manifestDirectory: dir }).exit_code_recommendation, 1); } finally { rmSync(dir, { recursive: true, force: true }); }
  assert.equal(evaluateSpecEvidenceManifest({ schema_version: 1, gates: [{ id: 'startup_matrix', report: 'x.json' }, { id: 'startup_matrix', report: 'x.json' }] }).exit_code_recommendation, 3);
});
test('markdown mirrors incomplete JSON status and collect-only does not rewrite it', () => {
  const { dir, manifest } = fixture({});
  const manifestPath = join(dir, 'manifest.json'); const outputPath = join(dir, 'audit.json'); const markdownPath = join(dir, 'audit.md');
  writeFileSync(manifestPath, JSON.stringify(manifest));
  try {
    assert.equal(runCli(['--manifest', manifestPath, '--output', outputPath, '--markdown', markdownPath, '--collect-only']), 0);
    const result = JSON.parse(readFileSync(outputPath, 'utf8'));
    const markdown = readFileSync(markdownPath, 'utf8');
    assert.equal(result.audit_status, 'incomplete');
    assert.equal(result.exit_code_recommendation, 2);
    assert.match(markdown, /Audit status: `incomplete`/);
    assert.match(markdown, /Spec completion: `not_evaluated`/);
    assert.match(markdown, /startup_matrix/);
    assert.equal(renderMarkdown(result), markdown);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

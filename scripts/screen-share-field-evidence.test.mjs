import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import {
  FIELD_EVIDENCE_GATES,
  evaluateFieldEvidence,
  parseArgs,
  renderMarkdown,
  runCli,
} from './screen-share-field-evidence.mjs';
import { FIELD_EVIDENCE_FIXTURES, distributionFixture } from './screen-share-field-evidence.fixtures.mjs';

function codes(result) {
  return result.issues.map((entry) => entry.code);
}

test('a bare scope plus status report never satisfies any field gate', () => {
  for (const [id, definition] of Object.entries(FIELD_EVIDENCE_GATES)) {
    const result = evaluateFieldEvidence(id, { scope: definition.scope, status: 'passed' });
    assert.equal(result.status, 'incomplete', `${id} accepted a two-field report`);
    assert.ok(result.issues.length > 1, `${id} produced no structural findings`);
  }
});

test('complete structured fixtures pass every field gate', () => {
  for (const [id, build] of Object.entries(FIELD_EVIDENCE_FIXTURES)) {
    const result = evaluateFieldEvidence(id, build(), { thresholds: undefined });
    assert.deepEqual(codes(result), [], `${id} unexpectedly reported ${JSON.stringify(result.issues)}`);
    assert.equal(result.status, 'passed');
  }
});

test('an explicit failed status or unknown gate is distinguished from missing evidence', () => {
  const failed = evaluateFieldEvidence('feature_regression', FIELD_EVIDENCE_FIXTURES.feature_regression({ status: 'failed' }));
  assert.equal(failed.status, 'failed');
  assert.equal(evaluateFieldEvidence('not_a_gate', {}).status, 'invalid');
  assert.equal(evaluateFieldEvidence('feature_regression', 'not-an-object').status, 'invalid');
});

test('performance matrix requires the full baseline matrix including the 60 FPS tier', () => {
  const only30 = FIELD_EVIDENCE_FIXTURES.performance_matrix();
  for (const run of only30.runs) run.fps = 30;
  assert.ok(codes(evaluateFieldEvidence('performance_matrix', only30)).includes('fps_tier_coverage_missing'));

  const singleRun = FIELD_EVIDENCE_FIXTURES.performance_matrix();
  singleRun.runs = [singleRun.runs[0]];
  const reduced = codes(evaluateFieldEvidence('performance_matrix', singleRun));
  assert.ok(reduced.includes('cpu_generation_coverage_missing'));
  assert.ok(reduced.includes('capture_backend_coverage_missing'));
  assert.ok(reduced.includes('scenario_coverage_missing'));
  assert.ok(reduced.includes('healthy_client_count_coverage_missing'));
});

test('percentiles without sample accounting or a trace source are not evidence', () => {
  const report = FIELD_EVIDENCE_FIXTURES.performance_matrix();
  report.runs[0].distributions.capture_to_display_ms = { p50: 30, p95: 60, p99: 90 };
  report.runs[1].distributions.live_edge_distance_ms = distributionFixture({ retained_sample_count: 4096, capacity: 512 });
  delete report.runs[2].presentation_trace_source;
  const found = codes(evaluateFieldEvidence('performance_matrix', report));
  assert.ok(found.includes('distribution_accounting_missing'));
  assert.ok(found.includes('distribution_retention_invalid'));
  assert.ok(found.includes('performance_trace_source_missing'));
});

test('independent device evidence rejects tab substitution, duplicates and slow reclaim', () => {
  const duplicated = FIELD_EVIDENCE_FIXTURES.independent_viewing_devices();
  duplicated.devices = duplicated.devices.slice(0, 20);
  duplicated.devices[3].id = duplicated.devices[2].id;
  const duplicateFindings = codes(evaluateFieldEvidence('independent_viewing_devices', duplicated));
  // 19 distinct identities behind 20 entries must not count as 20 devices.
  assert.ok(duplicateFindings.includes('device_id_duplicate'));
  assert.ok(duplicateFindings.includes('independent_device_count_insufficient'));

  const tabs = FIELD_EVIDENCE_FIXTURES.independent_viewing_devices({
    devices: Array.from({ length: 22 }, (_, index) => ({
      id: `tab-${index}`,
      os: 'Windows 10 22H2',
      browser: 'chrome',
      browser_version: '150.0.0.0',
      network_segment: '192.168.30.0/24',
      independent_hardware: false,
    })),
  });
  assert.ok(codes(evaluateFieldEvidence('independent_viewing_devices', tabs)).includes('device_not_independent'));

  const slow = FIELD_EVIDENCE_FIXTURES.independent_viewing_devices();
  slow.fanout_session.state_reclaim_seconds = 9;
  slow.fanout_session.healthy_lagged_frames = 12;
  const slowResult = evaluateFieldEvidence('independent_viewing_devices', slow);
  assert.equal(slowResult.status, 'failed');
  assert.ok(codes(slowResult).includes('fanout_state_reclaim_slow'));
  assert.ok(codes(slowResult).includes('fanout_healthy_lag_detected'));
});

test('managed browser evidence rejects loopback substitutes and unmanaged browsers', () => {
  const loopback = FIELD_EVIDENCE_FIXTURES.managed_browser_external_media({ synthetic_loopback_only: true });
  assert.equal(evaluateFieldEvidence('managed_browser_external_media', loopback).status, 'failed');

  const unmanaged = FIELD_EVIDENCE_FIXTURES.managed_browser_external_media({
    browsers: [{ name: 'chrome', version: '150.0.0.0', managed: false }],
    managed_browser_external_acceptance: false,
  });
  const unmanagedFindings = codes(evaluateFieldEvidence('managed_browser_external_media', unmanaged));
  assert.ok(unmanagedFindings.includes('managed_browser_missing'));
  assert.ok(unmanagedFindings.includes('managed_external_acceptance_missing'));

});

test('impairment thresholds come from the manifest and cannot be relaxed by the report', () => {
  const report = FIELD_EVIDENCE_FIXTURES.network_impairment_recovery();
  assert.equal(evaluateFieldEvidence('network_impairment_recovery', report).status, 'passed');

  const stricter = evaluateFieldEvidence('network_impairment_recovery', report, {
    thresholds: { maximum_recovery_p99_ms: 500, maximum_frame_gap_ms: 1500 },
  });
  assert.equal(stricter.status, 'failed');
  assert.ok(codes(stricter).includes('impairment_recovery_exceeded'));

  const missingThresholds = FIELD_EVIDENCE_FIXTURES.network_impairment_recovery({ thresholds: undefined });
  assert.ok(codes(evaluateFieldEvidence('network_impairment_recovery', missingThresholds)).includes('impairment_thresholds_missing'));

  const lossOnly = FIELD_EVIDENCE_FIXTURES.network_impairment_recovery();
  lossOnly.injections = [lossOnly.injections[0]];
  assert.ok(codes(evaluateFieldEvidence('network_impairment_recovery', lossOnly)).includes('impairment_kind_coverage_missing'));
});

test('transport selection requires MSE and WebRTC plus an explicit FPS decision', () => {
  const partial = FIELD_EVIDENCE_FIXTURES.transport_selection();
  partial.candidates = partial.candidates.slice(0, 1);
  assert.ok(codes(evaluateFieldEvidence('transport_selection', partial)).includes('transport_candidate_coverage_missing'));

  const noFpsDecision = FIELD_EVIDENCE_FIXTURES.transport_selection({ fps_default_decision: undefined });
  assert.ok(codes(evaluateFieldEvidence('transport_selection', noFpsDecision)).includes('fps_decision_missing'));

  const unjustifiedSwitch = FIELD_EVIDENCE_FIXTURES.transport_selection({
    decision: { selected: 'web_rtc', rationale: 'It felt smoother.' },
  });
  const switchFindings = codes(evaluateFieldEvidence('transport_selection', unjustifiedSwitch));
  assert.ok(switchFindings.includes('transport_replacement_unjustified'));
  assert.ok(switchFindings.includes('transport_operations_unaccepted'));

  const justifiedSwitch = FIELD_EVIDENCE_FIXTURES.transport_selection({
    decision: {
      selected: 'web_rtc',
      rationale: 'Lower capture-to-display P99 on all three generations.',
      improvement_over_mse: { significant: true, evidence: 'artifacts/screen-share-benchmarks/transport-compare.json' },
      operational_cost_acceptable: true,
    },
  });
  assert.equal(evaluateFieldEvidence('transport_selection', justifiedSwitch).status, 'passed');
});

test('feature regression requires every listed behaviour and both locales', () => {
  const missing = FIELD_EVIDENCE_FIXTURES.feature_regression();
  delete missing.checks.multi_monitor_switch;
  assert.ok(codes(evaluateFieldEvidence('feature_regression', missing)).includes('regression_check_missing'));

  const regressed = FIELD_EVIDENCE_FIXTURES.feature_regression();
  regressed.checks.cursor = { tested: true, passed: false };
  assert.equal(evaluateFieldEvidence('feature_regression', regressed).status, 'failed');

  const untranslated = FIELD_EVIDENCE_FIXTURES.feature_regression({ localization: { zh_cn: true, en_us: false } });
  assert.ok(codes(evaluateFieldEvidence('feature_regression', untranslated)).includes('regression_localization_missing'));
});

test('cli validates arguments and mirrors the JSON status into markdown', () => {
  assert.throws(() => parseArgs(['--report', 'x.json']), /--gate is required/);
  assert.throws(() => parseArgs(['--gate', 'nope', '--report', 'x.json']), /unknown gate/);
  assert.throws(() => parseArgs(['--gate', 'feature_regression']), /--report is required/);

  const dir = mkdtempSync(join(tmpdir(), 'field-evidence-'));
  try {
    const reportPath = join(dir, 'regression.json');
    const outputPath = join(dir, 'result.json');
    const markdownPath = join(dir, 'result.md');
    writeFileSync(reportPath, JSON.stringify(FIELD_EVIDENCE_FIXTURES.feature_regression({ localization: { zh_cn: true, en_us: false } })));

    assert.equal(runCli(['--gate', 'feature_regression', '--report', reportPath, '--output', outputPath, '--markdown', markdownPath]), 2);
    const result = JSON.parse(readFileSync(outputPath, 'utf8'));
    assert.equal(result.status, 'incomplete');
    assert.equal(result.spec_completion, 'not_evaluated');
    assert.equal(result.recommended_exit_code, 2);
    assert.equal(readFileSync(markdownPath, 'utf8'), renderMarkdown(result));

    // --collect-only only changes the process exit code, never the recorded result.
    assert.equal(runCli(['--gate', 'feature_regression', '--report', reportPath, '--output', outputPath, '--collect-only']), 0);
    assert.equal(JSON.parse(readFileSync(outputPath, 'utf8')).recommended_exit_code, 2);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

/**
 * Structured validators for the screen-share field-evidence reports that
 * `screen-share-spec-evidence-audit.mjs` treats as required gates.
 *
 * The startup qualification, M0 latency and WGC stability gates already emit
 * machine-generated JSON, so the audit can trust their `status`. The six field
 * categories below are authored by whoever runs the field session, so a bare
 * `{ scope, status: "passed" }` file must never be able to close the spec.
 * Every requirement here quotes a clause of
 * `docs/design/screen-share-latency-optimization.md`.
 */

import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

/** §3.3 baseline matrix: three target Intel generations. */
const REQUIRED_CPU_GENERATIONS = ['broadwell', 'skylake', 'intel-10th'];
/** §3.3: WGC main path, DXGI fallback, RDP, and Basic Display Adapter / no hardware encoder. */
const REQUIRED_CAPTURE_BACKENDS = ['wgc', 'dxgi', 'rdp', 'basic-display-adapter'];
/** §3.3: dynamic desktop, static desktop, video playback, fast scrolling. */
const REQUIRED_SCENARIOS = ['static', 'dynamic', 'video', 'fast-scroll'];
/** §3.3: 1, 5, 20 and 30 healthy viewers. */
const REQUIRED_HEALTHY_CLIENT_COUNTS = [1, 5, 20, 30];
/** §3.3: 720p30 and 1080p30 are mandatory, 60 FPS is the experiment tier of §6.2/§6.3. */
const REQUIRED_RESOLUTION_TIERS = ['720p30', '1080p30'];
const REQUIRED_FPS_TIERS = [30, 60];
/** §8.2: every stage distribution that each phase must report. */
const REQUIRED_RUN_DISTRIBUTIONS = [
  'capture_to_display_ms',
  'input_to_sendinput_ms',
  'input_to_visible_response_ms',
  'live_edge_distance_ms',
  'outbound_bitrate_100ms_bps',
  'outbound_bitrate_1s_bps',
  'idr_size_bytes',
  'fanout_send_ms',
  'input_queue_age_ms',
  'reconnect_recovery_ms',
];
/** §3.3: at least 20-30 independent devices for the final acceptance. */
const MINIMUM_INDEPENDENT_DEVICES = 20;
/** §4.6: healthy 30 clients for 30 minutes, state reclaimed within 3 seconds. */
const MINIMUM_FANOUT_DURATION_MINUTES = 30;
const MAXIMUM_STATE_RECLAIM_SECONDS = 3;
/** §7.4: the transports that must be compared under the same conditions. */
const REQUIRED_TRANSPORT_CANDIDATES = ['mse_h264', 'web_rtc'];
const REQUIRED_TRANSPORT_METRICS = [
  'capture_to_display_ms',
  'input_to_visible_response_ms',
  'recovery_after_impairment_ms',
];
/** §7.4/§3.3: loss and jitter injection are both required. */
const REQUIRED_IMPAIRMENT_KINDS = ['loss', 'jitter'];
/** §8.3 + §4.6 regression row. */
const REQUIRED_REGRESSION_CHECKS = [
  'annotations',
  'control_request_grant',
  'control_request_revoke',
  'keyboard_mouse_release_all',
  'cursor',
  'multi_monitor_switch',
  'privacy_black_screen_recovery',
  'wgc_backend',
  'dxgi_backend',
  'rdp_session',
  'software_encoder_fallback',
  'mjpeg_fallback',
];

function issue(code, message, severity = 'incomplete') {
  return { code, message, severity };
}

function isRecord(value) {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

function number(value) {
  return Number.isFinite(value);
}

function nonEmptyString(value) {
  return typeof value === 'string' && value.trim().length > 0;
}

function statusFor(issues) {
  if (issues.some((entry) => entry.severity === 'invalid')) return 'invalid';
  if (issues.some((entry) => entry.severity === 'failed')) return 'failed';
  return issues.length ? 'incomplete' : 'passed';
}

/**
 * §8.2: a percentile set is only evidence when it also reports how many samples
 * it came from, how many were retained, the window capacity, and its scope.
 */
function assessDistribution(label, value, issues) {
  if (!isRecord(value)) {
    issues.push(issue('distribution_missing', `${label} is missing`));
    return;
  }
  const percentiles = ['p50', 'p95', 'p99'];
  if (!percentiles.every((key) => number(value[key]))) {
    issues.push(issue('distribution_percentiles_missing', `${label} requires numeric p50/p95/p99`));
    return;
  }
  if (value.p50 > value.p95 || value.p95 > value.p99) {
    issues.push(issue('distribution_percentiles_inconsistent', `${label} percentiles must be non-decreasing`));
  }
  for (const key of ['sample_count', 'retained_sample_count', 'capacity']) {
    if (!number(value[key]) || value[key] < 0) {
      issues.push(issue('distribution_accounting_missing', `${label} requires a non-negative ${key}`));
    }
  }
  if (value.sample_count === 0) {
    issues.push(issue('distribution_empty', `${label} reports zero samples`));
  }
  if (number(value.retained_sample_count) && number(value.capacity) && value.retained_sample_count > value.capacity) {
    issues.push(issue('distribution_retention_invalid', `${label} retained_sample_count exceeds capacity`));
  }
  if (number(value.retained_sample_count) && number(value.sample_count) && value.retained_sample_count > value.sample_count) {
    issues.push(issue('distribution_retention_invalid', `${label} retained_sample_count exceeds sample_count`));
  }
  if (!nonEmptyString(value.measurement_scope)) {
    issues.push(issue('distribution_scope_missing', `${label} requires a measurement_scope`));
  }
}

function assessCoverage(label, observed, required, issues) {
  const seen = new Set(observed);
  const missing = required.filter((entry) => !seen.has(entry));
  if (missing.length) {
    issues.push(issue(`${label}_coverage_missing`, `${label} does not cover: ${missing.join(', ')}`));
  }
  return { covered: [...seen], missing };
}

/** §3.3 baseline matrix + §6.3 30/60 FPS reporting + §8.2 per-phase results. */
function assessPerformanceMatrix(report) {
  const issues = [];
  const runs = Array.isArray(report.runs) ? report.runs : [];
  if (!runs.length) {
    issues.push(issue('performance_runs_missing', 'performance matrix requires a runs array'));
    return { issues, checks: { run_count: 0 } };
  }

  const identifiers = new Set();
  runs.forEach((run, index) => {
    const label = nonEmptyString(run?.id) ? run.id : `run[${index}]`;
    if (!isRecord(run)) {
      issues.push(issue('performance_run_invalid', `${label} is not an object`));
      return;
    }
    if (!nonEmptyString(run.id)) issues.push(issue('performance_run_id_missing', `${label} requires an id`));
    else if (identifiers.has(run.id)) issues.push(issue('performance_run_id_duplicate', `${label} is duplicated`));
    else identifiers.add(run.id);

    for (const field of ['cpu_generation', 'resolution_tier', 'scenario', 'capture_backend', 'transport']) {
      if (!nonEmptyString(run[field])) issues.push(issue('performance_run_field_missing', `${label} requires ${field}`));
    }
    if (!number(run.fps) || run.fps <= 0) issues.push(issue('performance_run_field_missing', `${label} requires a positive fps`));
    if (!number(run.healthy_client_count) || run.healthy_client_count < 0) {
      issues.push(issue('performance_run_field_missing', `${label} requires healthy_client_count`));
    }
    if (!number(run.duration_minutes) || run.duration_minutes <= 0) {
      issues.push(issue('performance_run_field_missing', `${label} requires a positive duration_minutes`));
    }
    if (!nonEmptyString(run.presentation_trace_source)) {
      // §8.2: every presentation latency must carry its trace source.
      issues.push(issue('performance_trace_source_missing', `${label} requires presentation_trace_source`));
    }

    for (const name of REQUIRED_RUN_DISTRIBUTIONS) {
      assessDistribution(`${label}.${name}`, run.distributions?.[name], issues);
    }

    const frames = run.frame_accounting;
    if (!isRecord(frames) || !number(frames.presented_frames) || !number(frames.dropped_frames) || !number(frames.dropped_ratio)) {
      issues.push(issue('performance_frame_accounting_missing', `${label} requires presented_frames, dropped_frames and dropped_ratio`));
    } else if (frames.presented_frames <= 0 || frames.dropped_ratio < 0 || frames.dropped_ratio > 1) {
      issues.push(issue('performance_frame_accounting_invalid', `${label} frame accounting is out of range`));
    }

    const queue = run.input_queue;
    if (!isRecord(queue) || !number(queue.depth_max) || !number(queue.coalesced_count) || !number(queue.full_count)) {
      issues.push(issue('performance_input_queue_missing', `${label} requires input queue depth_max, coalesced_count and full_count`));
    }

    const resources = run.resource_usage;
    const resourceFields = ['host_cpu_percent', 'host_gpu_percent', 'host_memory_mb', 'viewer_cpu_percent', 'viewer_memory_mb'];
    if (!isRecord(resources) || !resourceFields.every((field) => number(resources[field]))) {
      issues.push(issue('performance_resource_usage_missing', `${label} requires ${resourceFields.join(', ')}`));
    }
  });

  const checks = {
    run_count: runs.length,
    cpu_generations: assessCoverage('cpu_generation', runs.map((run) => run?.cpu_generation), REQUIRED_CPU_GENERATIONS, issues),
    capture_backends: assessCoverage('capture_backend', runs.map((run) => run?.capture_backend), REQUIRED_CAPTURE_BACKENDS, issues),
    scenarios: assessCoverage('scenario', runs.map((run) => run?.scenario), REQUIRED_SCENARIOS, issues),
    resolution_tiers: assessCoverage('resolution_tier', runs.map((run) => run?.resolution_tier), REQUIRED_RESOLUTION_TIERS, issues),
    fps_tiers: assessCoverage('fps_tier', runs.map((run) => run?.fps), REQUIRED_FPS_TIERS, issues),
    healthy_client_counts: assessCoverage(
      'healthy_client_count',
      runs.map((run) => run?.healthy_client_count),
      REQUIRED_HEALTHY_CLIENT_COUNTS,
      issues,
    ),
  };
  return { issues, checks };
}

/** §3.3: 20-30 independent devices; §4.6: 30 healthy clients, 30 minutes, 3 second reclaim. */
function assessIndependentViewingDevices(report) {
  const issues = [];
  const devices = Array.isArray(report.devices) ? report.devices : [];
  const identifiers = new Set();
  devices.forEach((device, index) => {
    const label = nonEmptyString(device?.id) ? device.id : `device[${index}]`;
    if (!isRecord(device)) {
      issues.push(issue('device_entry_invalid', `${label} is not an object`));
      return;
    }
    for (const field of ['id', 'os', 'browser', 'browser_version', 'network_segment']) {
      if (!nonEmptyString(device[field])) issues.push(issue('device_field_missing', `${label} requires ${field}`));
    }
    if (device.independent_hardware !== true) {
      issues.push(issue('device_not_independent', `${label} must declare independent_hardware=true`));
    }
    if (nonEmptyString(device.id)) {
      if (identifiers.has(device.id)) issues.push(issue('device_id_duplicate', `${label} is duplicated`));
      else identifiers.add(device.id);
    }
  });
  if (identifiers.size < MINIMUM_INDEPENDENT_DEVICES) {
    issues.push(issue(
      'independent_device_count_insufficient',
      `${identifiers.size} independent devices is below the required ${MINIMUM_INDEPENDENT_DEVICES}`,
    ));
  }

  // Tabs on one machine are allowed for an early fan-out probe but never as a substitute.
  const substitution = report.tab_client_substitution;
  if (!isRecord(substitution) || !number(substitution.tab_client_count) || substitution.used_as_device_substitute !== false) {
    issues.push(issue(
      'tab_substitution_undeclared',
      'tab_client_substitution requires tab_client_count and used_as_device_substitute=false',
    ));
  }

  const fanout = report.fanout_session;
  if (!isRecord(fanout)) {
    issues.push(issue('fanout_session_missing', 'fanout_session is required'));
  } else {
    if (!number(fanout.duration_minutes) || fanout.duration_minutes < MINIMUM_FANOUT_DURATION_MINUTES) {
      issues.push(issue('fanout_duration_short', `fan-out duration must be at least ${MINIMUM_FANOUT_DURATION_MINUTES} minutes`));
    }
    if (!number(fanout.peak_concurrent_viewers) || fanout.peak_concurrent_viewers < MINIMUM_INDEPENDENT_DEVICES) {
      issues.push(issue('fanout_peak_viewers_insufficient', `peak_concurrent_viewers must reach ${MINIMUM_INDEPENDENT_DEVICES}`));
    }
    if (!number(fanout.healthy_lagged_frames)) {
      issues.push(issue('fanout_lag_missing', 'fanout_session requires healthy_lagged_frames'));
    } else if (fanout.healthy_lagged_frames > 0) {
      // §8.1: healthy clients must not lag; only the injected slow client may.
      issues.push(issue('fanout_healthy_lag_detected', `healthy clients lagged ${fanout.healthy_lagged_frames} frames`, 'failed'));
    }
    if (!number(fanout.state_reclaim_seconds)) {
      issues.push(issue('fanout_state_reclaim_missing', 'fanout_session requires state_reclaim_seconds'));
    } else if (fanout.state_reclaim_seconds > MAXIMUM_STATE_RECLAIM_SECONDS) {
      issues.push(issue(
        'fanout_state_reclaim_slow',
        `state reclaim ${fanout.state_reclaim_seconds}s exceeds ${MAXIMUM_STATE_RECLAIM_SECONDS}s`,
        'failed',
      ));
    }
  }

  return { issues, checks: { independent_device_count: identifiers.size, fanout_session: report.fanout_session ?? null } };
}

/** §7.1 certificate operations + §7.3 managed browser verification with real media. */
function assessManagedBrowserExternalMedia(report) {
  const issues = [];
  if (report.managed_browser_external_acceptance !== true) {
    issues.push(issue('managed_external_acceptance_missing', 'managed browser external acceptance is not explicitly true'));
  }
  if (report.synthetic_loopback_only === true) {
    issues.push(issue('managed_synthetic_only', 'same-browser synthetic loopback cannot satisfy this gate', 'failed'));
  }

  const browsers = Array.isArray(report.browsers) ? report.browsers : [];
  const managed = browsers.filter((browser) => isRecord(browser) && browser.managed === true);
  browsers.forEach((browser, index) => {
    const label = nonEmptyString(browser?.name) ? browser.name : `browser[${index}]`;
    if (!isRecord(browser) || !nonEmptyString(browser.name) || !nonEmptyString(browser.version)) {
      issues.push(issue('managed_browser_field_missing', `${label} requires name and version`));
      return;
    }
    if (browser.managed === true && !nonEmptyString(browser.policy_scope)) {
      issues.push(issue('managed_policy_scope_missing', `${label} requires policy_scope`));
    }
  });
  if (!managed.length) issues.push(issue('managed_browser_missing', 'at least one managed Chrome/Edge browser is required'));

  const transports = Array.isArray(report.transports) ? report.transports.filter(nonEmptyString) : [];
  if (!transports.length) issues.push(issue('managed_transports_missing', 'transports must list what was actually exercised'));

  const media = report.real_media_playback;
  if (!isRecord(media) || media.verified !== true || !number(media.rendered_frames) || media.rendered_frames <= 0) {
    issues.push(issue('managed_real_media_missing', 'real_media_playback requires verified=true and a positive rendered_frames'));
  }

  const peer = report.external_peer;
  if (!isRecord(peer) || peer.independent_host !== true || !nonEmptyString(peer.network_segment)) {
    issues.push(issue('managed_external_peer_missing', 'external_peer requires independent_host=true and network_segment'));
  }

  return { issues, checks: { managed_browser_count: managed.length, transports } };
}

/** §7.4: recovery time and frame continuity after loss/jitter injection. */
function assessNetworkImpairmentRecovery(report, thresholds) {
  const issues = [];
  const required = ['maximum_recovery_p99_ms', 'maximum_frame_gap_ms'];
  const resolved = isRecord(thresholds) ? thresholds : null;
  if (!resolved || !required.every((key) => number(resolved[key]) && resolved[key] >= 0)) {
    issues.push(issue('impairment_thresholds_missing', `thresholds require non-negative ${required.join(' and ')}`));
  }

  const injections = Array.isArray(report.injections) ? report.injections : [];
  if (!injections.length) issues.push(issue('impairment_injections_missing', 'injections are required'));
  injections.forEach((injection, index) => {
    const label = nonEmptyString(injection?.kind) ? injection.kind : `injection[${index}]`;
    if (!isRecord(injection)) {
      issues.push(issue('impairment_entry_invalid', `${label} is not an object`));
      return;
    }
    if (!nonEmptyString(injection.kind) || !nonEmptyString(injection.tool) || !nonEmptyString(injection.magnitude)) {
      issues.push(issue('impairment_field_missing', `${label} requires kind, magnitude and tool`));
    }
    assessDistribution(`${label}.recovery_ms`, injection.recovery_ms, issues);
    if (resolved && number(injection.recovery_ms?.p99) && injection.recovery_ms.p99 > resolved.maximum_recovery_p99_ms) {
      issues.push(issue(
        'impairment_recovery_exceeded',
        `${label} recovery p99=${injection.recovery_ms.p99}ms exceeds ${resolved.maximum_recovery_p99_ms}ms`,
        'failed',
      ));
    }
    const continuity = injection.frame_continuity;
    if (!isRecord(continuity) || continuity.recovered !== true || !number(continuity.max_gap_ms)) {
      issues.push(issue('impairment_continuity_missing', `${label} requires frame_continuity.recovered=true and max_gap_ms`));
    } else if (resolved && continuity.max_gap_ms > resolved.maximum_frame_gap_ms) {
      issues.push(issue(
        'impairment_frame_gap_exceeded',
        `${label} frame gap ${continuity.max_gap_ms}ms exceeds ${resolved.maximum_frame_gap_ms}ms`,
        'failed',
      ));
    }
  });

  assessCoverage('impairment_kind', injections.map((injection) => injection?.kind), REQUIRED_IMPAIRMENT_KINDS, issues);
  return { issues, checks: { injection_count: injections.length, thresholds: resolved } };
}

/** §7.4 same-condition comparison and §6.2/§6.3 default FPS tier decision. */
function assessTransportSelection(report) {
  const issues = [];
  const candidates = Array.isArray(report.candidates) ? report.candidates : [];
  const byTransport = new Map();
  candidates.forEach((candidate, index) => {
    const label = nonEmptyString(candidate?.transport) ? candidate.transport : `candidate[${index}]`;
    if (!isRecord(candidate) || !nonEmptyString(candidate.transport)) {
      issues.push(issue('transport_candidate_invalid', `${label} requires a transport`));
      return;
    }
    byTransport.set(candidate.transport, candidate);
    for (const metric of REQUIRED_TRANSPORT_METRICS) {
      assessDistribution(`${label}.${metric}`, candidate[metric], issues);
    }
    for (const field of ['outbound_bitrate_bps', 'host_cpu_percent', 'per_viewer_memory_mb', 'join_leave_idr_count']) {
      if (!number(candidate[field])) issues.push(issue('transport_candidate_field_missing', `${label} requires ${field}`));
    }
  });
  assessCoverage('transport_candidate', [...byTransport.keys()], REQUIRED_TRANSPORT_CANDIDATES, issues);

  const conditions = report.comparison_conditions;
  if (!isRecord(conditions)) {
    issues.push(issue('transport_conditions_missing', 'comparison_conditions are required'));
  } else {
    if (conditions.same_conditions !== true) {
      issues.push(issue('transport_conditions_not_equal', 'comparison_conditions.same_conditions must be true'));
    }
    if (!number(conditions.client_count) || conditions.client_count < 30) {
      issues.push(issue('transport_client_count_insufficient', 'comparison requires at least 30 clients'));
    }
    assessCoverage(
      'transport_cpu_generation',
      Array.isArray(conditions.cpu_generations) ? conditions.cpu_generations : [],
      REQUIRED_CPU_GENERATIONS,
      issues,
    );
  }

  const decision = report.decision;
  if (!isRecord(decision) || !nonEmptyString(decision.selected) || !nonEmptyString(decision.rationale)) {
    issues.push(issue('transport_decision_missing', 'decision requires selected and rationale'));
  } else if (!REQUIRED_TRANSPORT_CANDIDATES.includes(decision.selected)) {
    issues.push(issue('transport_decision_unknown', `decision.selected ${decision.selected} is not a compared transport`));
  } else if (decision.selected !== 'mse_h264') {
    // §7.4: only replace MSE when the data is clearly better and operations accept it.
    const improvement = decision.improvement_over_mse;
    if (!isRecord(improvement) || improvement.significant !== true || !nonEmptyString(improvement.evidence)) {
      issues.push(issue(
        'transport_replacement_unjustified',
        'replacing MSE requires improvement_over_mse.significant=true with evidence',
      ));
    }
    if (decision.operational_cost_acceptable !== true) {
      issues.push(issue('transport_operations_unaccepted', 'replacing MSE requires operational_cost_acceptable=true'));
    }
  }

  const fpsDecision = report.fps_default_decision;
  if (!isRecord(fpsDecision) || !REQUIRED_FPS_TIERS.includes(fpsDecision.selected_fps) || !nonEmptyString(fpsDecision.rationale)) {
    issues.push(issue('fps_decision_missing', 'fps_default_decision requires selected_fps of 30 or 60 and a rationale'));
  } else if (!Array.isArray(fpsDecision.evidence_run_ids) || !fpsDecision.evidence_run_ids.length) {
    issues.push(issue('fps_decision_evidence_missing', 'fps_default_decision requires evidence_run_ids from the performance matrix'));
  }

  return { issues, checks: { compared_transports: [...byTransport.keys()], decision: report.decision ?? null } };
}

/** §8.3 regression gate, including the localized user-facing copy. */
function assessFeatureRegression(report) {
  const issues = [];
  const checks = isRecord(report.checks) ? report.checks : null;
  if (!checks) {
    issues.push(issue('regression_checks_missing', 'checks object is required'));
  } else {
    for (const name of REQUIRED_REGRESSION_CHECKS) {
      const entry = checks[name];
      if (!isRecord(entry) || entry.tested !== true || typeof entry.passed !== 'boolean') {
        issues.push(issue('regression_check_missing', `${name} requires tested=true and an explicit passed boolean`));
      } else if (entry.passed !== true) {
        issues.push(issue('regression_check_failed', `${name} regressed`, 'failed'));
      }
    }
  }

  const localization = report.localization;
  if (!isRecord(localization) || localization.zh_cn !== true || localization.en_us !== true) {
    issues.push(issue('regression_localization_missing', 'localization requires zh_cn=true and en_us=true'));
  }

  return { issues, checks: { verified_checks: checks ? Object.keys(checks) : [], localization: localization ?? null } };
}

export const FIELD_EVIDENCE_GATES = {
  performance_matrix: { scope: 'performance-matrix', assess: assessPerformanceMatrix },
  independent_viewing_devices: { scope: 'independent-viewing-devices', assess: assessIndependentViewingDevices },
  managed_browser_external_media: { scope: 'managed-browser-external-media', assess: assessManagedBrowserExternalMedia },
  network_impairment_recovery: { scope: 'network-impairment-recovery', assess: assessNetworkImpairmentRecovery, usesThresholds: true },
  transport_selection: { scope: 'transport-selection', assess: assessTransportSelection },
  feature_regression: { scope: 'feature-regression', assess: assessFeatureRegression },
};

/**
 * Validates one field-evidence report. `thresholds` come from the audit manifest
 * when present so a report cannot lower its own bar.
 */
export function evaluateFieldEvidence(gateId, report, options = {}) {
  const definition = FIELD_EVIDENCE_GATES[gateId];
  if (!definition) {
    return { gate_id: gateId, status: 'invalid', issues: [issue('unknown_field_gate', `unknown field evidence gate: ${gateId}`, 'invalid')], checks: {} };
  }
  if (!isRecord(report)) {
    return { gate_id: gateId, status: 'invalid', issues: [issue('report_invalid', 'report must be a JSON object', 'invalid')], checks: {} };
  }
  const issues = [];
  if (report.scope !== definition.scope) {
    issues.push(issue('scope_mismatch', `expected scope ${definition.scope}, found ${report.scope ?? '(missing)'}`));
  }
  if (report.status === 'failed') issues.push(issue('report_failed', 'report declares status=failed', 'failed'));
  else if (report.status !== 'passed') issues.push(issue('report_status_missing', 'report does not declare status=passed'));
  if (!nonEmptyString(report.run_id) && !nonEmptyString(report.session_id)) {
    issues.push(issue('report_identity_missing', 'report requires a run_id or session_id'));
  }

  const thresholds = definition.usesThresholds
    ? { ...(isRecord(report.thresholds) ? report.thresholds : {}), ...(isRecord(options.thresholds) ? options.thresholds : {}) }
    : undefined;
  const assessment = definition.assess(report, thresholds);
  issues.push(...assessment.issues);

  return { gate_id: gateId, required_scope: definition.scope, status: statusFor(issues), issues, checks: assessment.checks };
}

export function parseArgs(argv) {
  const args = { collectOnly: false };
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index];
    if (token === '--collect-only') args.collectOnly = true;
    else if (['--gate', '--report', '--output', '--markdown', '--thresholds'].includes(token)) {
      const value = argv[++index];
      if (!value || value.startsWith('--')) throw new Error(`${token} requires a value`);
      args[token.slice(2)] = value;
    } else throw new Error(`unknown argument: ${token}`);
  }
  if (!args.gate) throw new Error('--gate is required');
  if (!Object.hasOwn(FIELD_EVIDENCE_GATES, args.gate)) throw new Error(`unknown gate: ${args.gate}`);
  if (!args.report) throw new Error('--report is required');
  return args;
}

export function renderMarkdown(result) {
  const rows = result.issues.map((entry) => `| ${entry.severity} | ${entry.code} | ${entry.message.replaceAll('|', '\\|')} |`).join('\n')
    || '| - | - | No missing or failing evidence. |';
  return `# Screen-share Field Evidence Gate

Gate: \`${result.gate_id}\`
Required scope: \`${result.required_scope ?? '(unknown)'}\`
Status: \`${result.status}\`
Spec completion: \`not_evaluated\`
Recommended exit code: \`${recommendedExitCode(result.status)}\`

## Findings

| Severity | Code | Detail |
| --- | --- | --- |
${rows}

A passing field-evidence report is one required input of the full-spec audit; it never declares the specification complete on its own.
`;
}

export function recommendedExitCode(status) {
  if (status === 'passed') return 0;
  if (status === 'failed') return 1;
  if (status === 'incomplete') return 2;
  return 3;
}

export function runCli(argv = process.argv.slice(2)) {
  const args = parseArgs(argv);
  const reportPath = resolve(args.report);
  if (!existsSync(reportPath)) throw new Error(`report does not exist: ${reportPath}`);
  let report;
  try {
    report = JSON.parse(readFileSync(reportPath, 'utf8'));
  } catch (error) {
    throw new Error(`report is not valid JSON: ${error.message}`);
  }
  let thresholds;
  if (args.thresholds) {
    try {
      thresholds = JSON.parse(readFileSync(resolve(args.thresholds), 'utf8'));
    } catch (error) {
      throw new Error(`thresholds file is not valid JSON: ${error.message}`);
    }
  }
  const result = { schema_version: 1, spec_completion: 'not_evaluated', ...evaluateFieldEvidence(args.gate, report, { thresholds }) };
  result.recommended_exit_code = recommendedExitCode(result.status);
  const payload = `${JSON.stringify(result, null, 2)}\n`;
  if (args.output) writeFileSync(resolve(args.output), payload, 'utf8');
  else process.stdout.write(payload);
  if (args.markdown) writeFileSync(resolve(args.markdown), renderMarkdown(result), 'utf8');
  return args.collectOnly ? 0 : result.recommended_exit_code;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    process.exitCode = runCli();
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 3;
  }
}

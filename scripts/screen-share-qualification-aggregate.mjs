import { createHash } from 'node:crypto';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, isAbsolute, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

export const SCOPE = 'target-host-startup-qualification';
export const REQUIRED_ROLES = [
  'm2:intel-broadwell',
  'm2:intel-skylake',
  'm2:intel-10th',
  'm3:intel',
  'm3:nvidia',
  'm3:amd',
];
const REQUIRED_STEP_NAMES = [
  'environment_inventory',
  'screen_share_web_build',
  'mf_system_memory_self_test',
  'gpu_dxgi_surface_self_test',
  'browser_capability_probe',
];

const EXTERNAL_EVIDENCE_REQUIRED = [
  'm0_end_to_end_latency_budget_and_correlated_timeline',
  'input_to_visible_pixel_or_optical_causality',
  'wgc_30_minute_stability_lock_multi_display_reconfigure_black_frame_recovery',
  '1080p_4k_and_fps_performance_matrix',
  'fanout_and_resources_on_20_to_30_independent_real_viewers',
  'managed_browser_real_webrtc_media_certificate_and_policy',
  'network_impairment_and_recovery',
  'end_to_end_transport_selection',
  'complete_functional_regression',
];

function issue(code, message, severity = 'incomplete') {
  return { code, message, severity };
}

function normalizeStatus(status) {
  return ['passed', 'failed', 'incomplete', 'invalid'].includes(status) ? status : 'invalid';
}

function stepOutcome(step) {
  if (!step || typeof step !== 'object' || typeof step.name !== 'string' || !step.name) {
    return { status: 'invalid', reason: 'step must have a non-empty name' };
  }
  if (step.status === 'passed') {
    if (Object.hasOwn(step, 'exit_code') && step.exit_code !== 0) {
      return { status: 'invalid', reason: `passed step ${step.name} has non-zero exit_code` };
    }
    if (Object.hasOwn(step, 'timed_out') && step.timed_out !== false) {
      return { status: 'invalid', reason: `passed step ${step.name} is timed_out` };
    }
    return { status: 'passed' };
  }
  if (step.status === 'failed' || step.status === 'timed_out' || step.timed_out === true) {
    return { status: 'failed', reason: step.reason || `step ${step.name} ${step.status}` };
  }
  if (step.status === 'skipped' || step.status === 'incomplete') {
    return { status: 'incomplete', reason: step.reason || `step ${step.name} ${step.status}` };
  }
  return { status: 'invalid', reason: `unknown step status for ${step.name}` };
}

function statusFromIssues(issues) {
  if (issues.some((entry) => entry.severity === 'invalid')) return 'invalid';
  if (issues.some((entry) => entry.severity === 'failed')) return 'failed';
  if (issues.length > 0) return 'incomplete';
  return 'passed';
}

function candidates(value) {
  if (Array.isArray(value)) return value;
  return value && typeof value === 'object' ? [value] : [];
}

function selfTestPasses(selfTest, requireGpuSurface) {
  if (!selfTest || typeof selfTest !== 'object') return false;
  return selfTest.attempted === true
    && selfTest.passed === true
    && selfTest.produced_access_units >= 1
    && selfTest.found_sps === true
    && selfTest.found_pps === true
    && selfTest.found_idr === true
    && selfTest.timeline_monotonic === true
    && selfTest.timestamps_from_encoder === true
    && selfTest.durations_from_encoder === true
    && selfTest.dynamic_pattern_input === true
    && selfTest.decoder_frame_count >= 1
    && (!requireGpuSurface || selfTest.gpu_surface_input === true)
    && selfTest.b_slice_count === 0
    && (selfTest.baseline_profile_confirmed === true
      || selfTest.b_frames_disabled?.value_matches === true);
}

function gateForCandidate(step, candidateList, gpu) {
  const prefix = gpu ? 'GPU DXGI surface' : 'system-memory Media Foundation';
  const stepResult = stepOutcome(step);
  if (stepResult.status !== 'passed') return { status: stepResult.status, reason: stepResult.reason || `${prefix} step did not pass` };
  if (candidateList.length === 0) return { status: 'incomplete', reason: `${prefix} candidate evidence is absent` };
  const admitted = candidateList.find((candidate) => candidate?.admitted === true && candidate?.hardware === true && candidate?.gpu_surface_input === gpu);
  if (!admitted) {
    return { status: 'incomplete', reason: `${prefix} has no admitted hardware candidate for the required input path` };
  }
  if (admitted.activation_succeeded !== true || admitted.configuration_succeeded !== true) {
    return { status: 'incomplete', reason: `${prefix} candidate does not assert successful activation and configuration` };
  }
  if (!selfTestPasses(admitted.self_test, gpu)) {
    return { status: 'incomplete', reason: `${prefix} candidate lacks a complete encoded-and-decoded self-test assertion` };
  }
  return { status: 'passed', candidate: admitted.name || null };
}

function v3GateEvidence(report, kind, gpu) {
  if (report.schema_version < 3) return { status: 'passed' };
  const evidence = report.media_foundation?.structured_evidence?.[kind];
  if (!evidence || typeof evidence !== 'object') return { status: 'incomplete', reason: `${kind} structured evidence is absent` };
  if (!Number.isInteger(evidence.candidate_total) || !Number.isInteger(evidence.candidate_parsed) || !Number.isInteger(evidence.candidate_malformed)) {
    return { status: 'incomplete', reason: `${kind} candidate parsing counts are absent` };
  }
  if (evidence.candidate_malformed !== 0 || evidence.candidate_total !== evidence.candidate_parsed) {
    return { status: 'incomplete', reason: `${kind} candidate evidence is malformed or incomplete` };
  }
  const assertions = evidence.gate_assertions;
  const adapter = report.host?.input_adapter;
  if (!adapter || !adapter.vendor_id || !adapter.device_id || !adapter.luid) {
    return { status: 'incomplete', reason: `${kind} selected input adapter identity is absent` };
  }
  if (!assertions || assertions.input_adapter_identity !== true || !assertions.activation_adapter_luid || assertions.luid_match !== true) {
    return { status: 'incomplete', reason: `${kind} activation adapter LUID match is not asserted` };
  }
  if (gpu) {
    const admitted = candidates(evidence.candidates).find((candidate) => candidate?.admitted === true && candidate?.gpu_surface_input === true);
    if (!admitted?.input_adapter?.luid || !admitted.activation_adapter_luid || admitted.luid_match !== true) {
      return { status: 'incomplete', reason: 'gpu_dxgi_surface admitted candidate lacks a matching normalized input/activation LUID' };
    }
    if (admitted.input_adapter.luid !== adapter.luid || admitted.activation_adapter_luid !== assertions.activation_adapter_luid) {
      return { status: 'incomplete', reason: 'gpu_dxgi_surface candidate adapter identity contradicts gate assertions' };
    }
  }
  if (gpu && assertions.pool_recycled !== true) return { status: 'incomplete', reason: 'gpu_dxgi_surface pool recycle is not asserted' };
  return { status: 'passed' };
}

function browserGate(step, report, reportPath) {
  const outcome = stepOutcome(step);
  if (outcome.status !== 'passed') return { status: outcome.status, reason: outcome.reason || 'browser constructor probe did not pass' };
  if (!report.browser_report) return { status: 'incomplete', reason: 'browser constructor probe report reference is absent' };
  const browserPath = resolveArtifact(report.browser_report, reportPath);
  if (!existsSync(browserPath)) return { status: 'incomplete', reason: 'browser constructor probe report is unavailable' };
  let browserReport;
  try {
    browserReport = JSON.parse(readFileSync(browserPath, 'utf8'));
  } catch {
    return { status: 'incomplete', reason: 'browser constructor probe report is not valid JSON' };
  }
  if (!Array.isArray(browserReport.browsers) || browserReport.browsers.length === 0) {
    return { status: 'incomplete', reason: 'browser constructor probe report has no browser results' };
  }
  if (!browserReport.browsers.some((browser) => browser?.result?.rtcPeerConnectionConstructed === true)) {
    return { status: 'incomplete', reason: 'browser constructor probe has no successful RTCPeerConnection construction' };
  }
  return { status: 'passed', scope: 'constructor_capability_only' };
}

function resolveArtifact(value, reportPath) {
  return isAbsolute(value) ? value : resolve(dirname(reportPath), value);
}

function sha256(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

function artifact(path, reportPath) {
  const resolved = resolveArtifact(path, reportPath);
  return {
    reference: path,
    resolved_path: resolved,
    exists: existsSync(resolved),
    sha256: existsSync(resolved) ? sha256(resolved) : null,
  };
}

function sourceEvidence(report) {
  const source = report.source || report.source_revision || report.git || {};
  return { commit: source.commit ?? source.git_commit, dirty: source.dirty ?? source.git_worktree_dirty, ...source };
}

function v3Artifacts(report, reportPath) {
  if (report.schema_version < 3) return [];
  const problems = [];
  const refs = [report.artifacts?.environment_report, report.artifacts?.browser_report, ...Object.values(report.artifacts?.step_logs || {})].filter(Boolean);
  if (refs.length < 2) return [issue('v3_artifacts_missing', 'v3 report does not provide artifact hash references')];
  for (const entry of refs) {
    if (!entry?.relative_path || !entry.sha256) { problems.push(issue('v3_artifact_hash_missing', 'v3 artifact lacks relative_path or sha256')); continue; }
    const path = resolve(dirname(reportPath), entry.relative_path);
    if (!existsSync(path)) { problems.push(issue('v3_artifact_missing', `v3 artifact is unavailable: ${entry.relative_path}`)); continue; }
    if (sha256(path) !== entry.sha256.toLowerCase()) problems.push(issue('v3_artifact_hash_mismatch', `v3 artifact hash mismatch: ${entry.relative_path}`, 'invalid'));
  }
  return problems;
}

function expectedMatches(run, report) {
  const expected = run.expected;
  if (!expected) return [];
  const observed = report.host || report.environment || report.machine || {};
  const problems = [];
  for (const key of ['hostname', 'cpu_name', 'gpu_vendor', 'gpu_pnp_device_id', 'input_adapter_luid']) {
    if (expected[key] === undefined) continue;
    if (observed[key] === undefined) problems.push(issue('expected_host_evidence_missing', `expected ${key} is declared but report does not provide it`));
    else if (String(observed[key]).toLowerCase() !== String(expected[key]).toLowerCase()) problems.push(issue('expected_host_mismatch', `expected ${key} does not match report`));
  }
  return problems;
}

function evaluateRun(run, manifest, options) {
  const reportPath = resolve(options.manifestDirectory, run.report);
  const base = { id: run.id, required: run.required !== false, roles: run.roles, report_path: reportPath };
  if (!existsSync(reportPath)) return { ...base, status: 'invalid', issues: [issue('report_missing', 'qualification report path does not exist', 'invalid')], gates: {}, artifacts: [] };
  let report;
  try { report = JSON.parse(readFileSync(reportPath, 'utf8')); } catch { return { ...base, status: 'invalid', issues: [issue('report_json_invalid', 'qualification report is not valid JSON', 'invalid')], gates: {}, artifacts: [artifact(reportPath, reportPath)] }; }
  const issues = [];
  if (!['passed', 'failed', 'incomplete'].includes(report.qualification_status)) {
    issues.push(issue('qualification_status_invalid', 'qualification_status must be passed, failed, or incomplete', 'invalid'));
  } else if (report.qualification_status !== 'passed') {
    issues.push(issue('qualification_status_reported', `source report declares qualification_status=${report.qualification_status}`, report.qualification_status));
  }
  if (!Array.isArray(report.steps)) issues.push(issue('steps_missing', 'qualification report has no steps array', 'invalid'));
  const steps = new Map();
  for (const step of report.steps || []) {
    if (!step?.name || steps.has(step.name)) { issues.push(issue('duplicate_step_name', `duplicate or invalid step name: ${step?.name || '(missing)'}`, 'invalid')); continue; }
    steps.set(step.name, step);
    const outcome = stepOutcome(step);
    if (outcome.status !== 'passed') issues.push(issue(`step_${outcome.status}`, `${step.name}: ${outcome.reason || outcome.status}`, outcome.status));
    if (outcome.status === 'passed' && (!step.log_path || !existsSync(resolveArtifact(step.log_path, reportPath)))) {
      issues.push(issue('step_log_missing', `${step.name}: passed step does not have an existing log artifact`));
    }
  }
  for (const name of REQUIRED_STEP_NAMES) if (!steps.has(name)) issues.push(issue('required_step_missing', `required step is absent: ${name}`));
  const mf = report.media_foundation || {};
  const systemCandidates = report.schema_version >= 3 ? candidates(mf.structured_evidence?.system_memory?.candidates) : candidates(mf.system_memory_candidate_reports);
  const gpuCandidates = report.schema_version >= 3 ? candidates(mf.structured_evidence?.gpu_dxgi_surface?.candidates) : candidates(mf.gpu_dxgi_candidate_reports);
  const gates = {
    system_memory_mf_startup: gateForCandidate(steps.get('mf_system_memory_self_test'), systemCandidates, false),
    gpu_dxgi_surface_startup: gateForCandidate(steps.get('gpu_dxgi_surface_self_test'), gpuCandidates, true),
    browser_constructor_probe: browserGate(steps.get('browser_capability_probe'), report, reportPath),
  };
  for (const [gate, kind, gpu] of [['system_memory_mf_startup', 'system_memory', false], ['gpu_dxgi_surface_startup', 'gpu_dxgi_surface', true]]) {
    const evidence = v3GateEvidence(report, kind, gpu);
    if (gates[gate].status === 'passed' && evidence.status !== 'passed') gates[gate] = evidence;
  }
  for (const [name, gate] of Object.entries(gates)) if (gate.status !== 'passed') issues.push(issue(`gate_${name}`, gate.reason || `${name} ${gate.status}`, gate.status));
  issues.push(...v3Artifacts(report, reportPath));
  issues.push(...expectedMatches(run, report));
  const source = sourceEvidence(report);
  if (manifest.expected_git_commit && source.commit !== manifest.expected_git_commit) issues.push(issue('commit_mismatch_or_missing', 'report commit does not match expected_git_commit'));
  if (options.requireClean && (source.commit === undefined || source.dirty === undefined || source.dirty === true)) issues.push(issue('clean_source_evidence_missing', 'required clean source commit/dirty evidence is absent or dirty'));
  const linked = [report.environment_report, report.browser_report, ...(report.steps || []).map((step) => step.log_path)].filter(Boolean);
  return {
    ...base,
    status: statusFromIssues(issues),
    qualification_status_observed: report.qualification_status ?? null,
    collect_only_observed: report.collect_only === true,
    gates,
    issues,
    artifacts: [artifact(reportPath, reportPath), ...linked.map((entry) => artifact(entry, reportPath))],
  };
}

function validateManifest(manifest) {
  const errors = [];
  if (!manifest || typeof manifest !== 'object' || manifest.schema_version !== 1) errors.push('manifest schema_version must be 1');
  if (!Array.isArray(manifest?.runs) || manifest.runs.length === 0) errors.push('manifest runs must be a non-empty array');
  const ids = new Set();
  for (const run of manifest?.runs || []) {
    if (!run || typeof run.id !== 'string' || !run.id || typeof run.report !== 'string' || !run.report || !Array.isArray(run.roles)) { errors.push('every run requires id, report, and roles'); continue; }
    if (ids.has(run.id)) errors.push(`duplicate run id: ${run.id}`);
    ids.add(run.id);
    for (const role of run.roles) if (!REQUIRED_ROLES.includes(role)) errors.push(`unsupported role: ${role}`);
  }
  return errors;
}

export function evaluateManifest(manifest, options = {}) {
  const errors = validateManifest(manifest);
  const manifestDirectory = options.manifestDirectory || process.cwd();
  if (errors.length) return resultForInvalid(errors);
  const runs = manifest.runs.map((run) => evaluateRun(run, manifest, { ...options, manifestDirectory }));
  const coverage = Object.fromEntries(REQUIRED_ROLES.map((role) => {
    const candidatesForRole = runs.filter((run) => run.required && run.roles.includes(role));
    const gate = role.startsWith('m2:') ? 'system_memory_mf_startup' : 'gpu_dxgi_surface_startup';
    const passed = candidatesForRole.some((run) => run.status === 'passed' && run.gates[gate]?.status === 'passed');
    return [role, { required_gate: gate, run_ids: candidatesForRole.map((run) => run.id), status: passed ? 'passed' : candidatesForRole.some((run) => run.status === 'failed') ? 'failed' : 'incomplete' }];
  }));
  const requiredRuns = runs.filter((run) => run.required);
  const observedFailures = requiredRuns.filter((run) => run.status === 'failed').flatMap((run) => run.issues.filter((entry) => entry.severity === 'failed').map((entry) => ({ run_id: run.id, ...entry })));
  const invalid = runs.some((run) => run.status === 'invalid');
  const failed = requiredRuns.some((run) => run.status === 'failed') || Object.values(coverage).some((entry) => entry.status === 'failed');
  const incomplete = requiredRuns.some((run) => run.status === 'incomplete') || Object.values(coverage).some((entry) => entry.status === 'incomplete');
  const startup_matrix_status = invalid ? 'invalid' : failed ? 'failed' : incomplete ? 'incomplete' : 'passed';
  const exit_code_recommendation = startup_matrix_status === 'passed' ? 0 : startup_matrix_status === 'failed' ? 1 : startup_matrix_status === 'incomplete' ? 2 : 3;
  return {
    schema_version: 1, scope: SCOPE, spec_completion: 'not_evaluated', validation_status: invalid ? 'invalid' : 'passed', startup_matrix_status, exit_code_recommendation,
    coverage, runs, observed_failures: observedFailures, external_evidence_required: EXTERNAL_EVIDENCE_REQUIRED, not_automatically_proven: EXTERNAL_EVIDENCE_REQUIRED,
    artifacts: runs.flatMap((run) => run.artifacts.map((entry) => ({ run_id: run.id, ...entry }))),
  };
}

function resultForInvalid(errors) {
  return { schema_version: 1, scope: SCOPE, spec_completion: 'not_evaluated', validation_status: 'invalid', startup_matrix_status: 'invalid', exit_code_recommendation: 3, coverage: {}, runs: [], observed_failures: errors.map((message) => issue('manifest_invalid', message, 'invalid')), external_evidence_required: EXTERNAL_EVIDENCE_REQUIRED, not_automatically_proven: EXTERNAL_EVIDENCE_REQUIRED, artifacts: [] };
}

export function parseArgs(argv) {
  const args = { collectOnly: false, requireClean: false };
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index];
    if (token === '--collect-only') args.collectOnly = true;
    else if (token === '--require-clean') args.requireClean = true;
    else if (['--manifest', '--output', '--markdown'].includes(token)) {
      const value = argv[++index];
      if (!value || value.startsWith('--')) throw new Error(`${token} requires a value`);
      args[token.slice(2)] = value;
    } else throw new Error(`unknown argument: ${token}`);
  }
  if (!args.manifest) throw new Error('--manifest is required');
  return args;
}

function markdown(result) {
  const rows = result.runs.map((run) => `| ${run.id} | ${run.status} | ${run.roles.join(', ')} |`).join('\n') || '| none | invalid | - |';
  return `# Target-host startup qualification matrix\n\nScope: \`${result.scope}\`  \nSpec completion: \`${result.spec_completion}\`  \nMatrix status: \`${result.startup_matrix_status}\` (recommended exit ${result.exit_code_recommendation})\n\n| Run | Status | Roles |\n| --- | --- | --- |\n${rows}\n\nThis report does not evaluate the complete screen-share specification.\n`;
}

export function runCli(argv = process.argv.slice(2)) {
  const args = parseArgs(argv);
  const manifestPath = resolve(args.manifest);
  let manifest;
  try { manifest = JSON.parse(readFileSync(manifestPath, 'utf8')); } catch (error) { throw new Error(`cannot read manifest: ${error.message}`); }
  const result = evaluateManifest(manifest, { manifestDirectory: dirname(manifestPath), requireClean: args.requireClean });
  const payload = `${JSON.stringify(result, null, 2)}\n`;
  if (args.output) writeFileSync(resolve(args.output), payload, 'utf8'); else process.stdout.write(payload);
  if (args.markdown) writeFileSync(resolve(args.markdown), markdown(result), 'utf8');
  return args.collectOnly ? 0 : result.exit_code_recommendation;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try { process.exitCode = runCli(); } catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 3; }
}

import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const RECOVERY_KINDS = ['lock_screen', 'display_reconfiguration'];
const HEX_SHA256 = /^[a-f0-9]{64}$/i;

function issue(code, message, severity = 'incomplete') { return { code, message, severity }; }
function readJson(path) { return JSON.parse(readFileSync(path, 'utf8')); }
function number(value) { return Number.isFinite(value); }
function isRecord(value) { return value && typeof value === 'object' && !Array.isArray(value); }

function validateManifest(manifest) {
  const errors = [];
  if (!isRecord(manifest) || manifest.schema_version !== 1) errors.push('manifest schema_version must be 1');
  if (!isRecord(manifest?.evidence) || typeof manifest.evidence.report !== 'string') errors.push('evidence requires report');
  const thresholds = manifest?.thresholds;
  if (!isRecord(thresholds)) errors.push('thresholds are required');
  else {
    for (const name of ['minimum_capture_duration_minutes', 'maximum_resource_growth_mb', 'maximum_handle_growth', 'minimum_monitors', 'maximum_black_frame_ratio', 'maximum_consecutive_black_frames', 'maximum_frame_gap_ms', 'minimum_frames_after_recovery']) {
      if (!number(thresholds[name]) || thresholds[name] < 0) errors.push(`thresholds.${name} must be a non-negative number`);
    }
    if (thresholds.minimum_capture_duration_minutes < 30) errors.push('thresholds.minimum_capture_duration_minutes must be at least 30');
    if (thresholds.minimum_monitors < 2) errors.push('thresholds.minimum_monitors must be at least 2');
    if (thresholds.minimum_frames_after_recovery < 1) errors.push('thresholds.minimum_frames_after_recovery must be at least 1');
    if (thresholds.maximum_black_frame_ratio > 1) errors.push('thresholds.maximum_black_frame_ratio must not exceed 1');
  }
  return errors;
}

function assessEvidence(config, thresholds, baseDirectory) {
  const path = resolve(baseDirectory, config.report);
  if (!existsSync(path)) return { status: 'invalid', issues: [issue('evidence_report_missing', 'WGC evidence report does not exist', 'invalid')], artifact: { report_path: path, exists: false }, checks: {} };
  let report;
  try { report = readJson(path); } catch { return { status: 'invalid', issues: [issue('evidence_report_json_invalid', 'WGC evidence report is not valid JSON', 'invalid')], artifact: { report_path: path, exists: true }, checks: {} }; }
  const issues = [];
  if (report.scope !== 'wgc-stability-recovery-evidence') issues.push(issue('evidence_scope_mismatch', `expected scope wgc-stability-recovery-evidence, found ${report.scope ?? '(missing)'}`));
  if (report.status === 'failed') issues.push(issue('evidence_report_failed', 'WGC evidence report explicitly failed', 'failed'));
  const checks = {};

  const stability = report.capture_stability;
  checks.capture_stability = stability ?? null;
  if (!isRecord(stability) || stability.completed !== true || !number(stability.duration_minutes) || !number(stability.unexpected_capture_failures)) issues.push(issue('capture_stability_missing', 'capture stability requires completed, duration_minutes, and unexpected_capture_failures'));
  else {
    if (stability.duration_minutes < thresholds.minimum_capture_duration_minutes) issues.push(issue('capture_duration_short', `capture duration ${stability.duration_minutes}m is below ${thresholds.minimum_capture_duration_minutes}m`));
    if (stability.unexpected_capture_failures > 0) issues.push(issue('capture_stability_failed', `unexpected capture failures=${stability.unexpected_capture_failures}`, 'failed'));
  }

  const resources = report.resource_growth;
  checks.resource_growth = resources ?? null;
  if (!isRecord(resources) || !number(resources.process_growth_mb) || !number(resources.handle_growth)) issues.push(issue('resource_growth_missing', 'resource growth requires process_growth_mb and handle_growth'));
  else {
    if (resources.unacceptable_leak === true) issues.push(issue('unacceptable_leak_detected', 'resource evidence declares an unacceptable leak', 'failed'));
    else if (resources.unacceptable_leak !== false) issues.push(issue('unacceptable_leak_missing', 'resource evidence must explicitly declare unacceptable_leak=false'));
    if (resources.process_growth_mb > thresholds.maximum_resource_growth_mb) issues.push(issue('resource_growth_exceeded', `process growth ${resources.process_growth_mb}MB exceeds ${thresholds.maximum_resource_growth_mb}MB`, 'failed'));
    if (resources.handle_growth > thresholds.maximum_handle_growth) issues.push(issue('handle_growth_exceeded', `handle growth ${resources.handle_growth} exceeds ${thresholds.maximum_handle_growth}`, 'failed'));
  }

  const recovery = report.recovery;
  checks.recovery = recovery ?? null;
  for (const kind of RECOVERY_KINDS) {
    const entry = recovery?.[kind];
    if (!isRecord(entry) || entry.tested !== true || entry.recovered !== true) issues.push(issue(`${kind}_recovery_missing`, `${kind} requires tested=true and recovered=true`));
    else if (entry.failed === true) issues.push(issue(`${kind}_recovery_failed`, `${kind} declares a recovery failure`, 'failed'));
  }

  const monitors = report.multi_monitor;
  checks.multi_monitor = monitors ?? null;
  if (!isRecord(monitors) || monitors.tested !== true || !number(monitors.covered_monitor_count)) issues.push(issue('multi_monitor_missing', 'multi-monitor coverage requires tested=true and covered_monitor_count'));
  else if (monitors.covered_monitor_count < thresholds.minimum_monitors) issues.push(issue('multi_monitor_insufficient', `covered monitors ${monitors.covered_monitor_count} is below ${thresholds.minimum_monitors}`));

  const blackFrames = report.black_frame_detection;
  checks.black_frame_detection = blackFrames ?? null;
  if (!isRecord(blackFrames) || !number(blackFrames.total_frames) || !number(blackFrames.detected_black_frames) || !number(blackFrames.max_consecutive_black_frames) || blackFrames.total_frames <= 0) issues.push(issue('black_frame_evidence_missing', 'black-frame evidence requires positive total_frames, detected_black_frames, and max_consecutive_black_frames'));
  else {
    const ratio = blackFrames.detected_black_frames / blackFrames.total_frames;
    checks.black_frame_detection = { ...blackFrames, observed_ratio: ratio };
    if (ratio > thresholds.maximum_black_frame_ratio) issues.push(issue('black_frame_ratio_exceeded', `black-frame ratio ${ratio} exceeds ${thresholds.maximum_black_frame_ratio}`, 'failed'));
    if (blackFrames.max_consecutive_black_frames > thresholds.maximum_consecutive_black_frames) issues.push(issue('black_frame_consecutive_exceeded', `consecutive black frames ${blackFrames.max_consecutive_black_frames} exceeds ${thresholds.maximum_consecutive_black_frames}`, 'failed'));
  }

  const events = report.recovery_event_accounting;
  checks.recovery_event_accounting = events ?? null;
  for (const kind of RECOVERY_KINDS) {
    const entry = events?.[kind];
    if (!isRecord(entry) || !number(entry.observed_count) || !number(entry.recovered_count) || !number(entry.failed_count)) issues.push(issue(`${kind}_event_accounting_missing`, `${kind} requires observed_count, recovered_count, and failed_count`));
    else if (entry.failed_count > 0) issues.push(issue(`${kind}_event_failed`, `${kind} failed events=${entry.failed_count}`, 'failed'));
    else if (entry.observed_count < 1 || entry.recovered_count !== entry.observed_count) issues.push(issue(`${kind}_event_recovery_incomplete`, `${kind} events must all be observed and recovered`));
  }

  const continuity = report.frame_continuity;
  checks.frame_continuity = continuity ?? null;
  if (!isRecord(continuity) || continuity.continuous !== true || !number(continuity.max_gap_ms) || !number(continuity.frames_after_recovery)) issues.push(issue('frame_continuity_missing', 'frame continuity requires continuous=true, max_gap_ms, and frames_after_recovery'));
  else {
    if (continuity.max_gap_ms > thresholds.maximum_frame_gap_ms) issues.push(issue('frame_gap_exceeded', `frame gap ${continuity.max_gap_ms}ms exceeds ${thresholds.maximum_frame_gap_ms}ms`, 'failed'));
    if (continuity.frames_after_recovery < thresholds.minimum_frames_after_recovery) issues.push(issue('post_recovery_frames_insufficient', `post-recovery frames ${continuity.frames_after_recovery} is below ${thresholds.minimum_frames_after_recovery}`));
  }

  const artifacts = report.external_artifacts;
  checks.external_artifacts = artifacts ?? null;
  if (!Array.isArray(artifacts) || artifacts.length === 0) issues.push(issue('external_artifacts_missing', 'external_artifacts must contain explicit artifact references'));
  else artifacts.forEach((artifact, index) => {
    if (!isRecord(artifact) || typeof artifact.id !== 'string' || typeof artifact.reference !== 'string' || !HEX_SHA256.test(artifact.sha256 ?? '')) issues.push(issue('external_artifact_invalid', `external artifact ${index} requires id, reference, and SHA-256`));
  });

  const status = issues.some((entry) => entry.severity === 'failed') ? 'failed' : issues.length ? 'incomplete' : 'passed';
  return { status, issues, artifact: { report_path: path, exists: true, scope: report.scope ?? null }, checks };
}

export function evaluateWgcStabilityManifest(manifest, options = {}) {
  const errors = validateManifest(manifest);
  if (errors.length) return invalidResult(errors);
  const evidence = assessEvidence(manifest.evidence, manifest.thresholds, options.manifestDirectory ?? process.cwd());
  const status = evidence.status;
  return {
    schema_version: 1, scope: 'wgc_stability_recovery', status, gate_status: status, spec_completion: 'not_evaluated',
    recommended_exit_code: status === 'passed' ? 0 : status === 'failed' ? 1 : status === 'incomplete' ? 2 : 3,
    thresholds: manifest.thresholds, evidence,
    gaps: evidence.issues.filter((entry) => entry.severity !== 'failed'), failures: evidence.issues.filter((entry) => entry.severity === 'failed'),
    limitations: ['Only structured, attributed WGC test evidence is accepted.', 'Unrelated logs, implementation claims, and fan-out metrics do not establish WGC stability or recovery.'],
  };
}

function invalidResult(errors) { return { schema_version: 1, scope: 'wgc_stability_recovery', status: 'invalid', gate_status: 'invalid', spec_completion: 'not_evaluated', recommended_exit_code: 3, thresholds: null, evidence: null, gaps: errors.map((message) => issue('manifest_invalid', message, 'invalid')), failures: [], limitations: [] }; }

export function renderMarkdown(result) {
  const gaps = result.gaps.map((entry) => `- ${entry.code}: ${entry.message}`).join('\n') || '- None.';
  const failures = result.failures.map((entry) => `- ${entry.code}: ${entry.message}`).join('\n') || '- None.';
  const checks = result.evidence?.checks ?? {};
  return `# WGC Stability And Recovery Gate\n\nScope: \`${result.scope}\`  \nStatus: \`${result.status}\`  \nSpec completion: \`${result.spec_completion}\`  \nRecommended exit code: \`${result.recommended_exit_code}\`\n\n## Evidence Artifact\n\n- Status: \`${result.evidence?.status ?? 'invalid'}\`\n- Artifact: \`${result.evidence?.artifact?.report_path ?? '(missing)'}\`\n\n## Required Coverage\n\n| Check | Observed evidence |\n| --- | --- |\n${Object.entries(checks).map(([name, value]) => `| ${name} | ${value ? 'present' : 'missing'} |`).join('\n') || '| none | missing |'}\n\n## Missing Or Incomplete Evidence\n\n${gaps}\n\n## Explicit Failures\n\n${failures}\n\nThis gate does not infer WGC stability from unrelated logs or implementation claims.\n`;
}

export function parseArgs(argv) { const args = { collectOnly: false }; for (let index = 0; index < argv.length; index += 1) { const token = argv[index]; if (token === '--collect-only') args.collectOnly = true; else if (['--manifest', '--output', '--markdown'].includes(token)) { const value = argv[++index]; if (!value || value.startsWith('--')) throw new Error(`${token} requires a value`); args[token.slice(2)] = value; } else throw new Error(`unknown argument: ${token}`); } if (!args.manifest) throw new Error('--manifest is required'); return args; }
export function runCli(argv = process.argv.slice(2)) { const args = parseArgs(argv); const manifestPath = resolve(args.manifest); let manifest; try { manifest = readJson(manifestPath); } catch (error) { throw new Error(`cannot read manifest: ${error.message}`); } const result = evaluateWgcStabilityManifest(manifest, { manifestDirectory: dirname(manifestPath) }); const payload = `${JSON.stringify(result, null, 2)}\n`; if (args.output) writeFileSync(resolve(args.output), payload, 'utf8'); else process.stdout.write(payload); if (args.markdown) writeFileSync(resolve(args.markdown), renderMarkdown(result), 'utf8'); return args.collectOnly ? 0 : result.recommended_exit_code; }
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) { try { process.exitCode = runCli(); } catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 3; } }

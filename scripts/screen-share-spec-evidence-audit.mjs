import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { FIELD_EVIDENCE_GATES, evaluateFieldEvidence } from './screen-share-field-evidence.mjs';

/**
 * The first three gates consume machine-generated gate output, so their own
 * tooling has already validated the measurements. The remaining six are field
 * reports written by whoever ran the session, so they are additionally checked
 * against `screen-share-field-evidence.mjs` and a declared `status` alone can
 * never close them.
 */
export const REQUIRED_GATES = {
  startup_matrix: { scope: 'target-host-startup-qualification', statusPath: 'startup_matrix_status' },
  m0_latency_input_visible: { scope: 'm0_latency_input_visible', statusPath: 'status' },
  wgc_stability_recovery: { scope: 'wgc_stability_recovery', statusPath: 'status' },
  ...Object.fromEntries(Object.entries(FIELD_EVIDENCE_GATES).map(([id, definition]) => [
    id,
    { scope: definition.scope, statusPath: 'status', fieldEvidence: id },
  ])),
};

function getPath(value, path) {
  return path.split('.').reduce((current, key) => current && typeof current === 'object' ? current[key] : undefined, value);
}

function issue(code, message, severity = 'incomplete') {
  return { code, message, severity };
}

function validateManifest(manifest) {
  const errors = [];
  if (!manifest || manifest.schema_version !== 1 || !Array.isArray(manifest.gates)) errors.push('manifest requires schema_version 1 and a gates array');
  const seen = new Set();
  for (const gate of manifest?.gates ?? []) {
    if (!gate || typeof gate.id !== 'string' || typeof gate.report !== 'string') { errors.push('every gate requires id and report'); continue; }
    if (!Object.hasOwn(REQUIRED_GATES, gate.id)) errors.push(`unknown gate: ${gate.id}`);
    if (seen.has(gate.id)) errors.push(`duplicate gate: ${gate.id}`);
    seen.add(gate.id);
  }
  return errors;
}

function assessGate(definition, reference, baseDirectory) {
  if (!reference) return { status: 'incomplete', issues: [issue('gate_missing', 'no report reference in manifest')], artifact: null };
  const reportPath = resolve(baseDirectory, reference.report);
  if (!existsSync(reportPath)) return { status: 'invalid', issues: [issue('report_missing', 'referenced report does not exist', 'invalid')], artifact: { report_path: reportPath, exists: false } };
  let report;
  try { report = JSON.parse(readFileSync(reportPath, 'utf8')); } catch { return { status: 'invalid', issues: [issue('report_json_invalid', 'referenced report is not valid JSON', 'invalid')], artifact: { report_path: reportPath, exists: true } }; }
  const artifact = { report_path: reportPath, exists: true, scope: report.scope ?? null };
  if (report.scope !== definition.scope) {
    return { status: 'incomplete', issues: [issue('scope_mismatch', `expected scope ${definition.scope}, found ${report.scope ?? '(missing)'}`)], artifact };
  }
  if (report.spec_completion === 'not_evaluated' && definition.scope === 'target-host-startup-qualification') {
    // A passed qualification matrix is deliberately a narrow prerequisite, not full spec completion.
  }
  if (definition.fieldEvidence) {
    // Thresholds live in the manifest so a field report cannot lower its own bar.
    const evidence = evaluateFieldEvidence(definition.fieldEvidence, report, { thresholds: reference.thresholds });
    return { status: evidence.status, issues: evidence.issues, artifact: { ...artifact, field_evidence_checks: evidence.checks } };
  }
  const status = getPath(report, definition.statusPath);
  if (status === 'passed') return { status: 'passed', issues: [], artifact };
  if (status === 'failed') return { status: 'failed', issues: [issue('reported_failure', `report declares ${definition.statusPath}=failed`, 'failed')], artifact };
  return { status: 'incomplete', issues: [issue('status_missing_or_incomplete', `report does not declare ${definition.statusPath}=passed`)], artifact };
}

export function evaluateSpecEvidenceManifest(manifest, options = {}) {
  const errors = validateManifest(manifest);
  if (errors.length) return invalidResult(errors);
  const refs = new Map(manifest.gates.map((gate) => [gate.id, gate]));
  const baseDirectory = options.manifestDirectory ?? process.cwd();
  const gates = Object.fromEntries(Object.entries(REQUIRED_GATES).map(([id, definition]) => {
    const assessment = assessGate(definition, refs.get(id), baseDirectory);
    return [id, { required_scope: definition.scope, ...assessment }];
  }));
  const statuses = Object.values(gates).map((gate) => gate.status);
  const overall = statuses.includes('invalid') ? 'invalid' : statuses.includes('failed') ? 'failed' : statuses.includes('incomplete') ? 'incomplete' : 'passed';
  const exit = overall === 'passed' ? 0 : overall === 'failed' ? 1 : overall === 'incomplete' ? 2 : 3;
  const supplementary = (manifest.supplementary ?? []).map((entry) => ({ ...entry, report_path: typeof entry?.report === 'string' ? resolve(baseDirectory, entry.report) : null }));
  return {
    schema_version: 1,
    scope: 'screen-share-full-spec-evidence-audit',
    spec_completion: overall === 'passed' ? 'passed' : 'not_evaluated',
    audit_status: overall,
    exit_code_recommendation: exit,
    gates,
    supplementary_evidence: supplementary,
    limitations: [
      'target-host startup qualification is a prerequisite only and never completes this audit alone',
      'browser constructor and same-browser WebRTC loopback evidence are supplementary only',
      'field evidence reports are structurally validated; a declared status alone never closes a gate',
    ],
  };
}

function invalidResult(errors) {
  return { schema_version: 1, scope: 'screen-share-full-spec-evidence-audit', spec_completion: 'not_evaluated', audit_status: 'invalid', exit_code_recommendation: 3, gates: {}, supplementary_evidence: [], limitations: errors };
}

export function parseArgs(argv) {
  const args = { collectOnly: false };
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index];
    if (token === '--collect-only') args.collectOnly = true;
    else if (token === '--manifest' || token === '--output' || token === '--markdown') {
      const value = argv[++index];
      if (!value || value.startsWith('--')) throw new Error(`${token} requires a value`);
      args[token.slice(2)] = value;
    } else throw new Error(`unknown argument: ${token}`);
  }
  if (!args.manifest) throw new Error('--manifest is required');
  return args;
}

function markdownCell(value) {
  return String(value ?? '').replaceAll('|', '\\|').replaceAll('\n', '<br>');
}

export function renderMarkdown(result) {
  const gateRows = Object.entries(result.gates).map(([id, gate]) => {
    const detail = gate.issues?.map((entry) => `${entry.code}: ${entry.message}`).join('; ') || 'passed';
    return `| ${markdownCell(id)} | ${markdownCell(gate.status)} | ${markdownCell(gate.required_scope)} | ${markdownCell(detail)} |`;
  }).join('\n') || '| none | invalid | - | no gate results |';
  const pending = Object.entries(result.gates).filter(([, gate]) => gate.status !== 'passed');
  const pendingRows = pending.map(([id, gate]) => `- \`${id}\`: requires \`${gate.required_scope}\` evidence (${gate.status}).`).join('\n') || '- None.';
  const supplementary = result.supplementary_evidence.map((entry) => `- \`${entry.kind ?? 'unspecified'}\`: \`${entry.report ?? '(no report)'}\`${entry.note ? ` - ${entry.note}` : ''}`).join('\n') || '- None.';
  return `# Screen-share Full-spec Evidence Audit

Scope: \`${result.scope}\`  
Audit status: \`${result.audit_status}\`  
Spec completion: \`${result.spec_completion}\`  
Recommended exit code: \`${result.exit_code_recommendation}\`

## Gate Summary

| Gate | Status | Required scope | Evidence result |
| --- | --- | --- | --- |
${gateRows}

## External Evidence Still Required

${pendingRows}

## Supplementary Evidence

${supplementary}

## Limits

${result.limitations.map((item) => `- ${item}`).join('\n')}

This audit does not claim full specification completion unless the JSON result has \`spec_completion=passed\`.
`;
}

export function runCli(argv = process.argv.slice(2)) {
  const args = parseArgs(argv);
  const manifestPath = resolve(args.manifest);
  let manifest;
  try { manifest = JSON.parse(readFileSync(manifestPath, 'utf8')); } catch (error) { throw new Error(`cannot read manifest: ${error.message}`); }
  const result = evaluateSpecEvidenceManifest(manifest, { manifestDirectory: dirname(manifestPath) });
  const payload = `${JSON.stringify(result, null, 2)}\n`;
  if (args.output) writeFileSync(resolve(args.output), payload, 'utf8'); else process.stdout.write(payload);
  if (args.markdown) writeFileSync(resolve(args.markdown), renderMarkdown(result), 'utf8');
  return args.collectOnly ? 0 : result.exit_code_recommendation;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try { process.exitCode = runCli(); } catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 3; }
}

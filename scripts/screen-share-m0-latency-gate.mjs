import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const REQUIRED_METRICS = ['capture_to_display_ms', 'input_to_sendinput_ms', 'input_to_visible_response_ms'];
const CAUSAL_METHODS = new Set(['pixel', 'optical', 'explicit_causal']);

function issue(code, message, severity = 'incomplete') { return { code, message, severity }; }
function getPath(value, path) { return path.split('.').reduce((current, key) => current && typeof current === 'object' ? current[key] : undefined, value); }
function readJson(path) { return JSON.parse(readFileSync(path, 'utf8')); }

function validateManifest(manifest) {
  const errors = [];
  if (!manifest || manifest.schema_version !== 1) errors.push('manifest schema_version must be 1');
  if (!manifest?.latency || typeof manifest.latency.report !== 'string' || typeof manifest.latency.thresholds_ms !== 'object') errors.push('latency requires report and thresholds_ms');
  if (!manifest?.input_to_visible || typeof manifest.input_to_visible.report !== 'string') errors.push('input_to_visible requires report');
  return errors;
}

function assessLatency(config, baseDirectory) {
  const path = resolve(baseDirectory, config.report);
  if (!existsSync(path)) return { status: 'invalid', issues: [issue('latency_report_missing', 'latency report does not exist', 'invalid')], artifact: { report_path: path, exists: false }, metrics: {} };
  let report;
  try { report = readJson(path); } catch { return { status: 'invalid', issues: [issue('latency_report_json_invalid', 'latency report is not valid JSON', 'invalid')], artifact: { report_path: path, exists: true }, metrics: {} }; }
  const issues = [];
  if (report.scope !== 'm0-latency-samples') issues.push(issue('latency_scope_mismatch', `expected scope m0-latency-samples, found ${report.scope ?? '(missing)'}`));
  if (report.status === 'failed') issues.push(issue('latency_report_failed', 'latency report explicitly failed', 'failed'));
  const metrics = {};
  for (const name of REQUIRED_METRICS) {
    const samples = report.metrics?.[name]; const thresholds = config.thresholds_ms?.[name];
    metrics[name] = { observed: samples ?? null, thresholds_ms: thresholds ?? null };
    if (!thresholds || !['p50', 'p95', 'p99'].every((percentile) => Number.isFinite(thresholds[percentile]))) {
      issues.push(issue('latency_threshold_missing', `${name} requires numeric p50/p95/p99 thresholds`)); continue;
    }
    if (!samples || !['p50', 'p95', 'p99'].every((percentile) => Number.isFinite(samples[percentile]))) {
      issues.push(issue('latency_percentile_missing', `${name} requires observed p50/p95/p99`)); continue;
    }
    for (const percentile of ['p50', 'p95', 'p99']) if (samples[percentile] > thresholds[percentile]) issues.push(issue('latency_threshold_exceeded', `${name} ${percentile}=${samples[percentile]} exceeds ${thresholds[percentile]}`, 'failed'));
  }
  return { status: issues.some((entry) => entry.severity === 'failed') ? 'failed' : issues.length ? 'incomplete' : 'passed', issues, artifact: { report_path: path, exists: true, scope: report.scope ?? null }, metrics };
}

function assessCausalEvidence(config, baseDirectory) {
  const path = resolve(baseDirectory, config.report);
  if (!existsSync(path)) return { status: 'invalid', issues: [issue('causal_report_missing', 'input-to-visible evidence report does not exist', 'invalid')], artifact: { report_path: path, exists: false }, evidence: null };
  let report;
  try { report = readJson(path); } catch { return { status: 'invalid', issues: [issue('causal_report_json_invalid', 'input-to-visible evidence report is not valid JSON', 'invalid')], artifact: { report_path: path, exists: true }, evidence: null }; }
  const issues = [];
  if (report.scope !== 'input-to-visible-causal-evidence') issues.push(issue('causal_scope_mismatch', `expected scope input-to-visible-causal-evidence, found ${report.scope ?? '(missing)'}`));
  if (report.status === 'failed') issues.push(issue('causal_report_failed', 'input-to-visible evidence explicitly failed', 'failed'));
  if (!CAUSAL_METHODS.has(report.method)) issues.push(issue('causal_method_missing', 'evidence method must be pixel, optical, or explicit_causal'));
  if (report.causal_link !== true) issues.push(issue('causal_link_missing', 'evidence must explicitly assert causal_link=true'));
  return { status: issues.some((entry) => entry.severity === 'failed') ? 'failed' : issues.length ? 'incomplete' : 'passed', issues, artifact: { report_path: path, exists: true, scope: report.scope ?? null }, evidence: report };
}

export function evaluateM0Manifest(manifest, options = {}) {
  const errors = validateManifest(manifest);
  if (errors.length) return invalidResult(errors);
  const baseDirectory = options.manifestDirectory ?? process.cwd();
  const latency = assessLatency(manifest.latency, baseDirectory);
  const inputToVisible = assessCausalEvidence(manifest.input_to_visible, baseDirectory);
  const statuses = [latency.status, inputToVisible.status];
  const status = statuses.includes('invalid') ? 'invalid' : statuses.includes('failed') ? 'failed' : statuses.includes('incomplete') ? 'incomplete' : 'passed';
  const recommended_exit_code = status === 'passed' ? 0 : status === 'failed' ? 1 : status === 'incomplete' ? 2 : 3;
  return {
    schema_version: 1, scope: 'm0_latency_input_visible', status, gate_status: status, spec_completion: 'not_evaluated', recommended_exit_code,
    latency, input_to_visible: inputToVisible,
    gaps: [...latency.issues, ...inputToVisible.issues].filter((entry) => entry.severity !== 'failed'),
    failures: [...latency.issues, ...inputToVisible.issues].filter((entry) => entry.severity === 'failed'),
    limitations: ['Latency percentiles do not prove input-to-visible causality.', 'Input-to-visible evidence must be pixel, optical, or an explicit controlled causal chain.'],
  };
}

function invalidResult(errors) { return { schema_version: 1, scope: 'm0_latency_input_visible', status: 'invalid', gate_status: 'invalid', spec_completion: 'not_evaluated', recommended_exit_code: 3, latency: null, input_to_visible: null, gaps: errors.map((message) => issue('manifest_invalid', message, 'invalid')), failures: [], limitations: [] }; }

export function renderMarkdown(result) {
  const metricRows = Object.entries(result.latency?.metrics ?? {}).map(([name, value]) => `| ${name} | ${value.observed ? `${value.observed.p50}/${value.observed.p95}/${value.observed.p99}` : 'missing'} | ${value.thresholds_ms ? `${value.thresholds_ms.p50}/${value.thresholds_ms.p95}/${value.thresholds_ms.p99}` : 'missing'} |`).join('\n') || '| none | missing | missing |';
  const gaps = result.gaps.map((entry) => `- ${entry.code}: ${entry.message}`).join('\n') || '- None.';
  const failures = result.failures.map((entry) => `- ${entry.code}: ${entry.message}`).join('\n') || '- None.';
  return `# M0 Latency And Input-to-visible Gate\n\nScope: \`${result.scope}\`  \nStatus: \`${result.status}\`  \nSpec completion: \`${result.spec_completion}\`  \nRecommended exit code: \`${result.recommended_exit_code}\`\n\n## Latency Metrics\n\n| Metric | Observed P50/P95/P99 ms | Threshold P50/P95/P99 ms |\n| --- | --- | --- |\n${metricRows}\n\n## Input-to-visible Evidence\n\n- Status: \`${result.input_to_visible?.status ?? 'invalid'}\`\n- Artifact: \`${result.input_to_visible?.artifact?.report_path ?? '(missing)'}\`\n\n## Missing Or Incomplete Evidence\n\n${gaps}\n\n## Explicit Failures\n\n${failures}\n\nLatency alone never proves input-to-visible causality.\n`;
}

export function parseArgs(argv) { const args = { collectOnly: false }; for (let index = 0; index < argv.length; index += 1) { const token = argv[index]; if (token === '--collect-only') args.collectOnly = true; else if (['--manifest', '--output', '--markdown'].includes(token)) { const value = argv[++index]; if (!value || value.startsWith('--')) throw new Error(`${token} requires a value`); args[token.slice(2)] = value; } else throw new Error(`unknown argument: ${token}`); } if (!args.manifest) throw new Error('--manifest is required'); return args; }
export function runCli(argv = process.argv.slice(2)) { const args = parseArgs(argv); const manifestPath = resolve(args.manifest); let manifest; try { manifest = readJson(manifestPath); } catch (error) { throw new Error(`cannot read manifest: ${error.message}`); } const result = evaluateM0Manifest(manifest, { manifestDirectory: dirname(manifestPath) }); const payload = `${JSON.stringify(result, null, 2)}\n`; if (args.output) writeFileSync(resolve(args.output), payload, 'utf8'); else process.stdout.write(payload); if (args.markdown) writeFileSync(resolve(args.markdown), renderMarkdown(result), 'utf8'); return args.collectOnly ? 0 : result.recommended_exit_code; }
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) { try { process.exitCode = runCli(); } catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 3; } }

# 屏幕共享现场证据门禁

> 对应 `screen-share-latency-optimization.md` 的 §3.3、§4.6、§6.2、§7.4、§8.2、§8.3。
>
> 本文描述 `screen-share-spec-evidence-audit` 要求的六类现场报告结构。它们由执行现场会话的人编写，因此必须结构化校验：只写 `{ "scope": ..., "status": "passed" }` 不能关闭任何门禁。

## 1. 适用范围

完整规格审计有九个门禁。其中三个消费工具自己生成的输出，已由各自门禁完成测量校验：

| 门禁 | 报告来源 |
|---|---|
| `startup_matrix` | `benchmark:screen-share:qualification:aggregate` |
| `m0_latency_input_visible` | `benchmark:screen-share:m0-gate` |
| `wgc_stability_recovery` | `benchmark:screen-share:wgc-stability-gate` |

其余六个是现场报告，由 `scripts/screen-share-field-evidence.mjs` 结构化校验：

```powershell
pnpm benchmark:screen-share:field-evidence -- --gate performance_matrix --report artifacts/screen-share-benchmarks/performance-matrix.json --output artifacts/screen-share-benchmarks/performance-matrix-gate.json --markdown artifacts/screen-share-benchmarks/performance-matrix-gate.md
```

退出码：`0` 通过、`1` 明确失败、`2` 证据缺失或不完整、`3` 参数或 JSON 无效。`--collect-only` 只让进程返回 0，不改写 JSON 中的 `status` 和 `recommended_exit_code`。

完整审计会对同一份报告重复执行该校验，因此单独运行只是提前发现问题，不是可以跳过的步骤：

```powershell
pnpm benchmark:screen-share:spec-evidence-audit -- --manifest artifacts/screen-share-benchmarks/full-spec-evidence.json --output artifacts/screen-share-benchmarks/full-spec-evidence-result.json --markdown artifacts/screen-share-benchmarks/full-spec-evidence-result.md
```

每份报告都必须声明 `scope`、`status` 和 `run_id`（或 `session_id`）。

## 2. 分布字段的统一要求

§8.2 要求任何百分位都必须能说明它来自多少样本。所有 `*_ms`、`*_bps`、`*_bytes` 分布字段结构固定：

```json
{
  "p50": 40, "p95": 80, "p99": 120,
  "sample_count": 1800,
  "retained_sample_count": 512,
  "capacity": 512,
  "measurement_scope": "rolling-window"
}
```

校验内容：p50 ≤ p95 ≤ p99；三个计数为非负数且 `sample_count > 0`；`retained_sample_count` 不得超过 `capacity` 或 `sample_count`；`measurement_scope` 非空。

## 3. `performance_matrix`（scope `performance-matrix`）

`runs[]` 每项必须有唯一 `id`，以及 `cpu_generation`、`resolution_tier`、`fps`、`scenario`、`capture_backend`、`transport`、`healthy_client_count`、`duration_minutes`、`presentation_trace_source`。

每个 run 的 `distributions` 必须包含 §8.2 的全部十项：`capture_to_display_ms`、`input_to_sendinput_ms`、`input_to_visible_response_ms`、`live_edge_distance_ms`、`outbound_bitrate_100ms_bps`、`outbound_bitrate_1s_bps`、`idr_size_bytes`、`fanout_send_ms`、`input_queue_age_ms`、`reconnect_recovery_ms`。

另需 `frame_accounting`（`presented_frames`、`dropped_frames`、`dropped_ratio`）、`input_queue`（`depth_max`、`coalesced_count`、`full_count`）和 `resource_usage`（`host_cpu_percent`、`host_gpu_percent`、`host_memory_mb`、`viewer_cpu_percent`、`viewer_memory_mb`）。

各 run 的并集必须覆盖 §3.3 基线矩阵和 §6.3 的双帧率报告：

| 维度 | 必须覆盖 |
|---|---|
| `cpu_generation` | `broadwell`、`skylake`、`intel-10th` |
| `capture_backend` | `wgc`、`dxgi`、`rdp`、`basic-display-adapter` |
| `scenario` | `static`、`dynamic`、`video`、`fast-scroll` |
| `resolution_tier` | `720p30`、`1080p30` |
| `fps` | `30`、`60` |
| `healthy_client_count` | `1`、`5`、`20`、`30` |

要求的是并集覆盖，不是笛卡尔积。60 FPS 由界面的“60 FPS（实验）”开关选择，默认档位仍是 30 FPS 及以下。

## 4. `independent_viewing_devices`（scope `independent-viewing-devices`）

`devices[]` 至少 20 个互不重复的 `id`，每项需要 `os`、`browser`、`browser_version`、`network_segment` 和 `independent_hardware: true`。同一台机器的多个标签页不算独立设备。

`tab_client_substitution` 必须显式声明 `tab_client_count` 和 `used_as_device_substitute: false`。

`fanout_session` 对应 §4.6：`duration_minutes ≥ 30`、`peak_concurrent_viewers ≥ 20`、`healthy_lagged_frames = 0`（大于 0 判为失败）、`state_reclaim_seconds ≤ 3`（超出判为失败）。

## 5. `managed_browser_external_media`（scope `managed-browser-external-media`）

- `managed_browser_external_acceptance: true`，且 `synthetic_loopback_only` 不得为 `true`（同浏览器 canvas loopback 只是补充证据）。
- `browsers[]` 至少一项 `managed: true` 并带 `policy_scope`；每项需要 `name` 和 `version`。
- `transports[]` 列出实际验证过的传输。
- `real_media_playback`：`verified: true` 且 `rendered_frames > 0`。
- `external_peer`：`independent_host: true` 和 `network_segment`。
- `secure_context` 的五个字段必须是显式布尔值：`https_terminated`、`certificate_trusted`、`certificate_rotation_tested`、`browser_profile_clear_tested`、`dhcp_ip_change_tested`。当 `transports` 含 `web_codecs` 时五项必须全部为 `true`；纯 `web_rtc` 明文会话可以如实填 `false`。

## 6. `network_impairment_recovery`（scope `network-impairment-recovery`）

`injections[]` 至少覆盖 `loss` 和 `jitter`，每项需要 `kind`、`magnitude`、`tool`、`recovery_ms` 分布和 `frame_continuity`（`recovered: true`、`max_gap_ms`）。

阈值 `maximum_recovery_p99_ms` 和 `maximum_frame_gap_ms` 可以写在报告的 `thresholds` 里，但审计 manifest 中同名字段优先级更高，报告不能放宽自己的阈值：

```json
{ "id": "network_impairment_recovery", "report": "impairment.json",
  "thresholds": { "maximum_recovery_p99_ms": 2000, "maximum_frame_gap_ms": 1000 } }
```

## 7. `transport_selection`（scope `transport-selection`）

- `candidates[]` 必须同时包含 `mse_h264`、`web_codecs`、`web_rtc`；每项需要 `capture_to_display_ms`、`input_to_visible_response_ms`、`recovery_after_impairment_ms` 三个分布，以及 `outbound_bitrate_bps`、`host_cpu_percent`、`per_viewer_memory_mb`、`join_leave_idr_count`。
- `comparison_conditions`：`same_conditions: true`、`client_count ≥ 30`、`cpu_generations` 覆盖三代目标 Intel。
- `decision`：`selected` 与 `rationale`。选择非 `mse_h264` 时，还必须有 `improvement_over_mse.significant: true`、对应 `evidence` 和 `operational_cost_acceptable: true`——§7.4 规定只有明显更优且可运营才替换 MSE。
- `fps_default_decision`：`selected_fps` 为 30 或 60、`rationale`，以及引用性能矩阵 run id 的 `evidence_run_ids`。

## 8. `feature_regression`（scope `feature-regression`）

`checks` 需要以下每一项，且都是 `{ "tested": true, "passed": true }`；`passed: false` 判为失败：

`annotations`、`control_request_grant`、`control_request_revoke`、`keyboard_mouse_release_all`、`cursor`、`multi_monitor_switch`、`privacy_black_screen_recovery`、`wgc_backend`、`dxgi_backend`、`rdp_session`、`software_encoder_fallback`、`mjpeg_fallback`。

`localization` 需要 `zh_cn: true` 和 `en_us: true`（§8.3 中英文文案同步）。

## 9. 结构合法不等于测量真实

该门禁校验的是报告结构、覆盖范围和阈值比较。它无法判断数字是否真的来自目标硬件，因此现场报告仍必须保留原始 JSON、日志和外部产物引用。六个门禁全部通过只是 `spec_completion=passed` 的必要条件之一，`scripts/screen-share-field-evidence.fixtures.mjs` 中的样例仅供测试使用，不得作为测量结果提交。

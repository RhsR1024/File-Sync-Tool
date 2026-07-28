export interface NumericMetricSnapshot {
  sampleCount: number;
  retainedSampleCount: number;
  last: number | null;
  min: number | null;
  max: number | null;
  average: number | null;
  p50: number | null;
  p95: number | null;
  p99: number | null;
}

function percentile(sorted: number[], fraction: number): number | null {
  if (sorted.length === 0) return null;
  const index = Math.min(sorted.length - 1, Math.max(0, Math.ceil(sorted.length * fraction) - 1));
  return sorted[index];
}

/** Fixed-size samples keep browser-side telemetry bounded during long sessions. */
export class RollingNumericMetric {
  private readonly values: number[] = [];
  private nextIndex = 0;
  private totalSamples = 0;
  private lastValue: number | null = null;

  constructor(private readonly capacity = 512) {}

  add(value: number): void {
    if (!Number.isFinite(value) || this.capacity <= 0) return;
    this.totalSamples += 1;
    this.lastValue = value;
    if (this.values.length < this.capacity) {
      this.values.push(value);
      return;
    }
    this.values[this.nextIndex] = value;
    this.nextIndex = (this.nextIndex + 1) % this.capacity;
  }

  snapshot(): NumericMetricSnapshot {
    const sorted = [...this.values].sort((left, right) => left - right);
    const retainedTotal = sorted.reduce((sum, value) => sum + value, 0);
    return {
      sampleCount: this.totalSamples,
      retainedSampleCount: sorted.length,
      last: this.lastValue,
      min: sorted[0] ?? null,
      max: sorted.at(-1) ?? null,
      average: sorted.length === 0 ? null : retainedTotal / sorted.length,
      p50: percentile(sorted, 0.5),
      p95: percentile(sorted, 0.95),
      p99: percentile(sorted, 0.99),
    };
  }
}

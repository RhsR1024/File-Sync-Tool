import type { SinglePortResult } from './tauri';

export type PortTableFilter = 'all' | 'open' | 'closed';
export type PortGridState = 'open' | 'closed' | 'scanning' | 'waiting';

export interface PortGridCell {
  port: number;
  state: PortGridState;
  latencyMs: number | null;
  name: string;
}

const MIN_PORT = 1;
const MAX_PORT = 65535;

export function parsePorts(input: string): number[] {
  const normalized = input.trim().toLowerCase();
  if (normalized === 'all' || normalized === '*') {
    return range(MIN_PORT, MAX_PORT);
  }

  const parts = input.split(',').map(s => s.trim()).filter(Boolean);
  const ports: Set<number> = new Set();
  for (const part of parts) {
    if (part.includes('-')) {
      const [startStr, endStr] = part.split('-');
      const start = Number.parseInt(startStr, 10);
      const end = Number.parseInt(endStr, 10);
      if (Number.isInteger(start) && Number.isInteger(end) && start >= MIN_PORT && end <= MAX_PORT && start <= end) {
        for (let i = start; i <= end; i++) ports.add(i);
      }
    } else {
      const port = Number.parseInt(part, 10);
      if (Number.isInteger(port) && port >= MIN_PORT && port <= MAX_PORT) ports.add(port);
    }
  }
  return Array.from(ports).sort((a, b) => a - b);
}

export function buildPortGridCells(
  ports: readonly number[],
  rows: ReadonlyMap<number, SinglePortResult>,
  isScanning: boolean,
): PortGridCell[] {
  return ports.map((port) => {
    const row = rows.get(port);
    if (row) {
      return {
        port,
        state: row.open ? 'open' : 'closed',
        latencyMs: row.latencyMs,
        name: row.name,
      };
    }

    return {
      port,
      state: isScanning ? 'scanning' : 'waiting',
      latencyMs: null,
      name: '',
    };
  });
}

export function filterPortRows(
  rows: ReadonlyMap<number, SinglePortResult>,
  filter: PortTableFilter,
): SinglePortResult[] {
  return Array.from(rows.values())
    .filter((row) => filter === 'all' || (filter === 'open' ? row.open : !row.open))
    .sort((a, b) => a.port - b.port);
}

function range(start: number, end: number): number[] {
  const values: number[] = [];
  for (let i = start; i <= end; i++) values.push(i);
  return values;
}

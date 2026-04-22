import type { ClipboardGroup } from './clipboardTypes.ts';

export interface ClipboardPanelGroupLabels {
  defaultGroup: string;
  createGroup: string;
}

export type ClipboardPanelGroupRow =
  | {
      kind: 'group';
      id: number | null;
      name: string;
      selected: boolean;
      isDefault: boolean;
      showSeparatorAbove: boolean;
    }
  | {
      kind: 'create';
      label: string;
      showSeparatorAbove: boolean;
    };

export function resolveClipboardPanelGroupLabel(
  groups: ClipboardGroup[],
  selectedGroupId: number | null,
  defaultLabel: string,
): string {
  if (selectedGroupId === null) {
    return defaultLabel;
  }

  return groups.find((group) => group.id === selectedGroupId)?.name ?? defaultLabel;
}

export function buildClipboardPanelGroupRows(
  groups: ClipboardGroup[],
  selectedGroupId: number | null,
  labels: ClipboardPanelGroupLabels,
): ClipboardPanelGroupRow[] {
  const rows: ClipboardPanelGroupRow[] = [
    {
      kind: 'group',
      id: null,
      name: labels.defaultGroup,
      selected: selectedGroupId === null,
      isDefault: true,
      showSeparatorAbove: false,
    },
  ];

  groups.forEach((group, index) => {
    rows.push({
      kind: 'group',
      id: group.id,
      name: group.name,
      selected: selectedGroupId === group.id,
      isDefault: false,
      showSeparatorAbove: index === 0,
    });
  });

  rows.push({
    kind: 'create',
    label: labels.createGroup,
    showSeparatorAbove: true,
  });

  return rows;
}

export interface ClipboardSelectionState {
  selectedIds: Set<number>;
  anchorId: number | null;
}

export interface ClipboardSelectionToggleInput extends ClipboardSelectionState {
  visibleIds: number[];
  targetId: number;
  shiftKey?: boolean;
}

export function toggleClipboardSelection(
  input: ClipboardSelectionToggleInput,
): ClipboardSelectionState {
  const targetIndex = input.visibleIds.indexOf(input.targetId);
  if (targetIndex < 0) {
    return {
      selectedIds: new Set(input.selectedIds),
      anchorId: input.anchorId,
    };
  }

  const nextSelectedIds = new Set(input.selectedIds);
  const anchorIndex =
    input.anchorId === null ? -1 : input.visibleIds.indexOf(input.anchorId);

  if (input.shiftKey && anchorIndex >= 0) {
    const start = Math.min(anchorIndex, targetIndex);
    const end = Math.max(anchorIndex, targetIndex);
    for (let index = start; index <= end; index += 1) {
      nextSelectedIds.add(input.visibleIds[index]);
    }
  } else if (nextSelectedIds.has(input.targetId)) {
    nextSelectedIds.delete(input.targetId);
  } else {
    nextSelectedIds.add(input.targetId);
  }

  return {
    selectedIds: nextSelectedIds,
    anchorId: input.targetId,
  };
}

export function pruneClipboardSelection(
  visibleIds: number[],
  state: ClipboardSelectionState,
): ClipboardSelectionState {
  const visibleIdSet = new Set(visibleIds);
  const nextSelectedIds = new Set(
    Array.from(state.selectedIds).filter((id) => visibleIdSet.has(id)),
  );

  return {
    selectedIds: nextSelectedIds,
    anchorId:
      state.anchorId !== null && visibleIdSet.has(state.anchorId)
        ? state.anchorId
        : null,
  };
}

export function resolveQuickPasteTargetId<T extends { id: number }>(
  items: T[],
  key: string,
  altKey: boolean,
): number | null {
  if (!altKey) return null;
  if (!/^[1-9]$/.test(key)) return null;

  const index = Number(key) - 1;
  return items[index]?.id ?? null;
}

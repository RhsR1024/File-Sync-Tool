import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

const pageSource = readFileSync(
  resolve(process.cwd(), 'src/pages/PaperTodoWindowPage.vue'),
  'utf8',
);
const backendSource = readFileSync(
  resolve(process.cwd(), 'src-tauri/src/paper_todo.rs'),
  'utf8',
);
const styleSource = readFileSync(
  resolve(process.cwd(), 'src/style.css'),
  'utf8',
);
const paperSource = readFileSync(
  resolve(process.cwd(), 'src/components/paper-todo/PaperTodoPaper.vue'),
  'utf8',
);
const launcherSource = readFileSync(
  resolve(process.cwd(), 'src/pages/PaperTodoLauncherPage.vue'),
  'utf8',
);
const mainSource = readFileSync(
  resolve(process.cwd(), 'src-tauri/src/main.rs'),
  'utf8',
);

describe('paper todo standalone window lifecycle', () => {
  it('keeps the native window hidden until the paper route is ready', () => {
    expect(backendSource).toMatch(/\.visible\(false\)/);
    expect(pageSource).toMatch(/await currentWindow\.show\(\)/);
  });

  it('uses a transparent document canvas and exposes a closeable error state', () => {
    expect(pageSource).toContain("const PAPER_WINDOW_CLASS = 'paper-todo-window'");
    expect(styleSource).toMatch(/html\.paper-todo-window,[\s\S]*background:\s*transparent/);
    expect(pageSource).toMatch(/v-else-if="store\.error\.value"/);
    expect(pageSource).toMatch(/@click="getCurrentWindow\(\)\.close\(\)"/);
  });

  it('renders editable paper bodies instead of placing them in an inert template', () => {
    expect(paperSource).not.toMatch(/<template>\s*<div v-if="paper\.kind === 'todo'"/);
    expect(paperSource).toContain('v-model="newTodoText"');
    expect(paperSource).toContain('@input="changeNote"');
  });

  it('fills the transparent window with a rounded borderless paper surface', () => {
    expect(pageSource).not.toContain('bg-transparent p-1');
    expect(paperSource).toContain('class="paper-surface');
    expect(paperSource).toMatch(/\.paper-surface \{[\s\S]*?border:\s*0;[\s\S]*?clip-path:/);
    expect(backendSource).toMatch(/\.transparent\(true\)[\s\S]*?\.shadow\(false\)/);
  });

  it('creates runtime windows away from the WebView callback thread', () => {
    expect(backendSource).toMatch(/pub async fn paper_todo_create_paper[\s\S]*?spawn_blocking/);
    expect(backendSource).toMatch(/pub async fn paper_todo_open_window[\s\S]*?spawn_blocking/);
    expect(backendSource).toMatch(/pub async fn paper_todo_set_all_windows[\s\S]*?spawn_blocking/);
    expect(mainSource).toContain('paper_todo::dispatch_background(app.clone(), "newTodo")');
    expect(mainSource).toContain('paper_todo::dispatch_background(app.clone(), "newNote")');
  });

  it('keeps deletion non-modal and closes only after persistence returns', () => {
    const deleteCommand = backendSource.slice(
      backendSource.indexOf('pub fn paper_todo_delete_paper'),
      backendSource.indexOf('pub fn paper_todo_close_window'),
    );
    expect(deleteCommand).not.toContain('.close()');
    expect(paperSource).not.toContain("window.confirm(t('paperTodo.confirmDeletePaper'))");
    expect(paperSource).toContain('await store.removePaper(id)');
  });

  it('deletes an untouched paper on close and saves authored papers explicitly', () => {
    const closeHandler = paperSource.slice(
      paperSource.indexOf('async function closeDesktop'),
      paperSource.indexOf('function cancelPeekTimer'),
    );
    expect(closeHandler).toContain('isPaperEmpty(current)');
    expect(closeHandler).toContain('await store.removePaper(id)');
    expect(closeHandler).toContain('await store.flush()');
    expect(paperSource).toMatch(
      /event\.key\.toLowerCase\(\) === 's'[\s\S]*?event\.preventDefault\(\)[\s\S]*?saveCurrentPaper/,
    );
  });

  it('provides dedicated drag handles for the edge launcher and paper window', () => {
    expect(launcherSource).toContain('launcher-drag-handle');
    expect(launcherSource).toContain('await dragPaperLauncher()');
    expect(paperSource).toContain('paper-window-drag-handle');
    expect(paperSource).toContain("dockCapsule('nearest')");
    expect(paperSource).toMatch(
      /async function dockCapsule[\s\S]*?invoke<string>\('paper_todo_dock_window'/,
    );
  });

  it('makes the whole master capsule the drag handle instead of a grip icon', () => {
    expect(launcherSource).toContain('class="launcher-master-capsule launcher-drag-handle"');
    expect(launcherSource).toContain('@mousedown.stop.prevent="startLauncherDrag"');
    expect(launcherSource).not.toContain('GripVertical');
    expect(launcherSource).not.toContain('launcher-master-drag');
    expect(launcherSource).not.toContain('launcher-master-toggle');

    // A press cannot be classified in the webview: the native loop reports
    // whether it travelled, and a press that never moved is the toggle.
    const dragHandler = launcherSource.slice(
      launcherSource.indexOf('async function startLauncherDrag'),
      launcherSource.indexOf('function toggleFromKeyboard'),
    );
    expect(dragHandler).toContain('await setExpanded(!expanded.value)');
    // Keyboard activation never reaches the drag loop, so it still toggles.
    expect(launcherSource).toContain('if (event.detail !== 0) return;');
    expect(launcherSource).toContain('@click="toggleFromKeyboard"');
  });

  it('confines the launcher drag to the primary display edge', () => {
    expect(backendSource).toContain('pub async fn paper_todo_drag_launcher');
    expect(mainSource).toContain('paper_todo::paper_todo_drag_launcher');
    const dragLoop = backendSource.slice(
      backendSource.indexOf('fn run_launcher_drag(app: &AppHandle)'),
      backendSource.indexOf('pub async fn paper_todo_drag_launcher'),
    );
    // `x` stays at the docked edge and `y` never leaves the primary monitor.
    expect(dragLoop).toContain('PhysicalPosition::new(origin.x, target)');
    expect(dragLoop).toContain('.clamp(min_y, max_y)');
    expect(dragLoop).toContain('LAUNCHER_DRAG_THRESHOLD');
    expect(dragLoop).toContain('LAUNCHER_DRAG_MAX_MS');
    expect(dragLoop).not.toContain('start_dragging');
  });

  it('sizes the collapsed launcher window to the capsule label', () => {
    expect(backendSource).toContain('fn collapsed_launcher_width');
    expect(backendSource).toContain('const LAUNCHER_EDGE_OVERHANG: u32 = 8;');
    expect(backendSource).not.toContain('LAUNCHER_VISIBLE_WIDTH');
    expect(backendSource).toContain('let visible_width = (window_width - overhang).max(1);');
    // Expanding shows a different label, so no width is reported then and the
    // last collapsed measurement stands.
    expect(launcherSource).toContain('value ? null : measureCapsuleWidth()');
    expect(launcherSource).toContain('width: max-content');
  });

  it('reserves height for the creation row instead of clipping its bottom edge', () => {
    expect(backendSource).toContain('const LAUNCHER_EXPANDED_SLACK: u32 = 4;');
    expect(backendSource).toMatch(
      /LAUNCHER_CAPSULE_HEIGHT\)\)\s*\.saturating_add\(LAUNCHER_EXPANDED_SLACK\)/,
    );
    // An intrinsic empty-state height does not match the row the backend
    // reserves for it, and the overflow lands on the creation buttons.
    expect(launcherSource).toMatch(/\.launcher-empty \{[\s\S]*?flex: 0 0 26px;/);
  });

  it('creates both paper kinds from the launcher and suppresses webview context menus', () => {
    expect(launcherSource).toContain("@click=\"createPaper('todo')\"");
    expect(launcherSource).toContain("@click=\"createPaper('note')\"");
    expect(launcherSource).toContain('ListPlus');
    expect(launcherSource).toContain('FilePlus2');
    expect(launcherSource).toContain('@contextmenu.prevent');
    expect(paperSource).toContain('@contextmenu.prevent');
  });

  it('keeps both creation actions visible when the launcher has no papers', () => {
    expect(launcherSource).toContain('paperCount.value === 0 ? 2 : paperCount.value + 1');
    expect(launcherSource).toContain('const itemCount = expandedRowCount.value;');
    expect(launcherSource).toMatch(
      /v-if="paperCount === 0"[\s\S]*?launcher-create-actions/,
    );
  });

  it('offers a direct delete control for every launcher paper row', () => {
    expect(launcherSource).toContain('class="launcher-paper-delete"');
    expect(launcherSource).toContain('@click.stop="deletePaper(paper)"');
    expect(launcherSource).toContain('await store.removePaper(paper.id)');
    expect(launcherSource).toContain('await closePaperWindow(paper.id)');
    expect(mainSource).toContain('paper_todo::paper_todo_close_window');
  });

  it('sizes launcher rows to their titles and caps visible titles at ten characters', () => {
    expect(launcherSource).toContain('characters.length > 10');
    expect(launcherSource).toContain('characters.slice(0, 9)');
    expect(launcherSource).toContain('paperTitleIsTruncated(paper.title) ? paper.title : undefined');
    expect(launcherSource).toContain('width: fit-content');
    expect(launcherSource).toContain('.launcher-paper-slot:hover');
    expect(launcherSource).not.toContain('width: 176px');
    expect(launcherSource).not.toContain('setPaperLauncherRowHovered');
    expect(launcherSource).toMatch(
      /\.launcher-paper-delete \{[\s\S]*?opacity: 0;[\s\S]*?pointer-events: none;/,
    );
  });

  it('reserves the full native width whenever the launcher is expanded', () => {
    expect(backendSource).toContain('const LAUNCHER_COLLAPSED_WIDTH: u32 = 96;');
    expect(backendSource).toContain('const LAUNCHER_EXPANDED_WIDTH: u32 = 184;');
    expect(backendSource).toContain('let width = if expanded {');
    expect(backendSource).not.toContain('launcher_row_hovered');
    expect(backendSource).toContain('tokio::time::Duration::from_millis(150)');
    expect(backendSource).toContain('delayed launcher size sync failed');
  });

  it('folds a paper into a capsule rather than a shrunken window', () => {
    expect(backendSource).toContain('const CAPSULE_WIDTH: f64 = 216.0;');
    expect(backendSource).toContain('const CAPSULE_HEIGHT: f64 = 40.0;');
    // The min size has to move with the mode or Windows refuses to shrink the
    // window down to the capsule.
    expect(backendSource).toMatch(/set_min_size[\s\S]*?CAPSULE_WIDTH/);
    // The capsule draws its own surface; it must not reuse the expanded header.
    expect(paperSource).toContain('class="paper-capsule"');
    expect(paperSource).toContain('paper-capsule-spine-fill');
    expect(paperSource).toContain('border-radius: 999px');
  });

  it('rests a docked capsule at the display edge and slides it back on hover', () => {
    expect(backendSource).toContain('pub fn paper_todo_set_edge_peek');
    expect(backendSource).toContain('const CAPSULE_PEEK_VISIBLE: f64 = 14.0;');
    // Docking and mode changes must supersede an in-flight slide.
    expect(backendSource).toMatch(/pub fn paper_todo_dock_window[\s\S]*?cancel_capsule_slide/);
    expect(backendSource).toMatch(/pub fn paper_todo_set_window_mode[\s\S]*?cancel_capsule_slide/);
    expect(paperSource).toContain('@mouseenter="onCapsuleEnter"');
    expect(paperSource).toContain('@mouseleave="onCapsuleLeave"');
  });

  it('never persists a window origin that the app moved itself', () => {
    expect(pageSource).toContain('if (store.geometryTrackingSuspended.value) return;');
    expect(paperSource).toContain('store.suspendGeometryTracking(400)');
  });

  it('pulls an expanding capsule back inside its monitor', () => {
    expect(backendSource).toContain('fn pull_window_into_monitor');
    expect(backendSource).toMatch(/if !collapsed \{[\s\S]*?pull_window_into_monitor/);
  });

  it('expands the launcher vertically on the primary monitor', () => {
    expect(launcherSource).toContain('id="paper-todo-capsule-list"');
    expect(launcherSource).toContain('class="launcher-paper-list"');
    expect(launcherSource).toContain('flex-direction: column');
    expect(backendSource).toContain('const LAUNCHER_EXPANDED_HEIGHT: u32 = 360;');

    const launcherPosition = backendSource.slice(
      backendSource.indexOf('fn launcher_position'),
      backendSource.indexOf('fn sync_launcher_window'),
    );
    expect(launcherPosition.indexOf('.primary_monitor()')).toBeLessThan(
      launcherPosition.indexOf('.current_monitor()'),
    );
    expect(launcherPosition).toContain('collapsed_available_height');
    expect(launcherPosition).toContain('let y = anchored_y.min(max_y);');
  });

  it('uses PaperTodo-style count and arrow cues, then persists drag ordering', () => {
    expect(launcherSource).toContain("t('paperTodo.launcher.collapsedCount', { count: paperCount })");
    expect(launcherSource).toContain('ChevronRight');
    expect(launcherSource).toContain('ChevronDown');
    expect(launcherSource).toContain('draggable="true"');
    expect(launcherSource).toContain('store.reorderPapers(orderedIds)');
    expect(launcherSource).toContain('movePaperId(');
    expect(launcherSource).toContain('openPaperWindow(paper, store.settings.value)');
  });

  it('keeps the launcher open after an explicit expand', () => {
    // Expanding moves and resizes the window under a stationary cursor, and the
    // webview reports that as a `mouseleave`. Arming the collapse timer on it
    // folded the launcher back up 700 ms later, before the list was ever read.
    // A boolean guard could not survive it, because `startDragging` flushes the
    // pointer events its modal loop swallowed at the same moment and the first
    // stray one cleared the guard; the leave has to be ignored on a deadline.
    expect(launcherSource).toMatch(
      /function scheduleCollapse[\s\S]*?if \(Date\.now\(\) < settleUntil\) return;/,
    );
    expect(launcherSource).toContain('settleUntil = Date.now() + SETTLE_MS;');
    // The collapse itself re-checks the pointer, so a leave that is followed by
    // the cursor coming back never folds the list away underneath it.
    expect(launcherSource).toMatch(
      /collapseTimer = setTimeout\(\(\) => \{[\s\S]*?if \(pointerInside/,
    );
    // Enter alone is not enough: a resize under a still cursor may never fire
    // one, so movement has to re-arm auto-collapse too.
    expect(launcherSource).toContain('@mouseenter="noteLauncherHovered"');
    expect(launcherSource).toContain('@mousemove="noteLauncherHovered"');
    // Native resize commands are dispatched on command threads. Preserve the
    // UI action order so startup's width report cannot win after the first
    // expand.
    expect(launcherSource).toContain('let launcherSyncQueue: Promise<void> = Promise.resolve();');
    expect(launcherSource).toContain('launcherSyncQueue = sync.catch(() => undefined);');
    const launcherMount = launcherSource.slice(
      launcherSource.indexOf('onMounted(async () =>'),
      launcherSource.lastIndexOf('onBeforeUnmount('),
    );
    expect(launcherMount).not.toContain('await setExpanded(false);');
    expect(launcherMount).toContain('await syncCollapsedWidth();');
  });

  it('keeps the edge launcher in the topmost band while it is on screen', () => {
    // `always_on_top` only sets HWND_TOPMOST once, so any other app that raises
    // its own topmost window covered the capsule for the rest of the session.
    expect(backendSource).toContain('fn reassert_topmost(window: &tauri::WebviewWindow)');
    expect(backendSource).toContain('HWND_TOPMOST');
    expect(backendSource).toContain('SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER');
    expect(backendSource).toMatch(
      /if reassert_paper_topmost\(&app, false\) \{[\s\S]*?interval = LAUNCHER_TOPMOST_REFRESH_MS;/,
    );
  });

  it('reopens every saved paper when show all is requested', () => {
    expect(backendSource).toContain('prepare_papers_for_show_all(&mut document)');
    expect(backendSource).toContain('paper["desktopOpen"] = json!(true)');
    // One unopenable paper must not strand the rest, so failures are collected
    // rather than propagated with `?` out of the loop.
    expect(backendSource).toContain('if let Err(error) = open_window_internal(app, paper, settings.clone())');
    expect(backendSource).toContain('failures.push(error)');
  });

  it('rescues papers restored onto a monitor that is no longer connected', () => {
    expect(backendSource).toContain('!window_is_on_screen(&window)');
    expect(backendSource).toContain('fn rect_is_on_any_screen');
  });

  it('hides live paper windows without destroying their webviews', () => {
    const setAllWindows = backendSource.slice(
      backendSource.indexOf('fn set_all_windows_internal'),
      backendSource.indexOf('fn prepare_papers_for_show_all'),
    );
    expect(setAllWindows).toContain('window.hide()');
    expect(setAllWindows).not.toContain('window.close()');
  });
});

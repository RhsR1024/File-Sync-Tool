import { defineComponent, h } from 'vue';

interface IconRender {
  viewBox?: string;
  draw: () => ReturnType<typeof h>;
}

const ICONS: Record<string, IconRender> = {
  folder: {
    draw: () => h('path', {
      d: 'M3 6.5C3 5.67 3.67 5 4.5 5h4.79c.4 0 .78.16 1.06.44L11.7 6.79c.28.28.66.44 1.06.44h6.74c.83 0 1.5.67 1.5 1.5v9.77c0 .83-.67 1.5-1.5 1.5h-15A1.5 1.5 0 0 1 3 18.5V6.5Z',
      fill: 'currentColor',
    }),
  },
  download: {
    draw: () => h('path', {
      d: 'M12 4v10m0 0 4-4m-4 4-4-4M5 18.5h14',
      stroke: 'currentColor',
      'stroke-width': 1.8,
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
      fill: 'none',
    }),
  },
  upload: {
    draw: () => h('path', {
      d: 'M12 20V8m0 0-4 4m4-4 4 4M5 5.5h14',
      fill: 'none',
      stroke: 'currentColor',
      'stroke-width': 1.8,
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
    }),
  },
  newfolder: {
    draw: () => h('g', { fill: 'none', stroke: 'currentColor', 'stroke-width': 1.8, 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }, [
      h('path', { d: 'M3 7c0-.83.67-1.5 1.5-1.5h4.79l2.13 2.13H19.5c.83 0 1.5.67 1.5 1.5v9.37c0 .83-.67 1.5-1.5 1.5h-15A1.5 1.5 0 0 1 3 18.5V7Z' }),
      h('path', { d: 'M12 11v6m3-3h-6' }),
    ]),
  },
  text: {
    draw: () => h('g', { fill: 'none', stroke: 'currentColor', 'stroke-width': 1.8, 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }, [
      h('path', { d: 'M6 4h8l4 4v12a1.5 1.5 0 0 1-1.5 1.5h-10A1.5 1.5 0 0 1 5 20V5.5A1.5 1.5 0 0 1 6 4Z' }),
      h('path', { d: 'M14 4v4h4M8 13h8M8 17h5' }),
    ]),
  },
  preview: {
    draw: () => h('g', { fill: 'none', stroke: 'currentColor', 'stroke-width': 1.8, 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }, [
      h('path', { d: 'M2.5 12s3.5-6 9.5-6 9.5 6 9.5 6-3.5 6-9.5 6S2.5 12 2.5 12Z' }),
      h('circle', { cx: 12, cy: 12, r: 3 }),
    ]),
  },
  edit: {
    draw: () => h('path', {
      d: 'M14.5 5 19 9.5 8.5 20H4v-4.5L14.5 5Z',
      fill: 'none',
      stroke: 'currentColor',
      'stroke-width': 1.8,
      'stroke-linejoin': 'round',
    }),
  },
  trash: {
    draw: () => h('path', {
      d: 'M5 7h14M9 7V5.5A1.5 1.5 0 0 1 10.5 4h3A1.5 1.5 0 0 1 15 5.5V7m-6 4v6m6-6v6M6 7h12l-1 12.5A1.5 1.5 0 0 1 15.5 21h-7A1.5 1.5 0 0 1 7 19.5L6 7Z',
      fill: 'none',
      stroke: 'currentColor',
      'stroke-width': 1.8,
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
    }),
  },
  search: {
    draw: () => h('g', { fill: 'none', stroke: 'currentColor', 'stroke-width': 1.8, 'stroke-linecap': 'round' }, [
      h('circle', { cx: 11, cy: 11, r: 6.5 }),
      h('path', { d: 'm20 20-4-4' }),
    ]),
  },
  refresh: {
    draw: () => h('path', {
      d: 'M4 12a8 8 0 0 1 13.7-5.6L20 9M20 4v5h-5M20 12a8 8 0 0 1-13.7 5.6L4 15m0 5v-5h5',
      fill: 'none',
      stroke: 'currentColor',
      'stroke-width': 1.8,
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
    }),
  },
  home: {
    draw: () => h('path', {
      d: 'm3.5 11 8.5-7 8.5 7M5.5 9.5V20h5v-5h3v5h5V9.5',
      fill: 'none',
      stroke: 'currentColor',
      'stroke-width': 1.8,
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
    }),
  },
  check: {
    draw: () => h('path', {
      d: 'm5 11 4 4 10-10',
      fill: 'none',
      stroke: 'currentColor',
      'stroke-width': 2.4,
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
    }),
  },
  list: {
    draw: () => h('g', { fill: 'none', stroke: 'currentColor', 'stroke-width': 1.8, 'stroke-linecap': 'round' }, [
      h('path', { d: 'M8 6h12M8 12h12M8 18h12' }),
      h('circle', { cx: 4, cy: 6, r: 1, fill: 'currentColor' }),
      h('circle', { cx: 4, cy: 12, r: 1, fill: 'currentColor' }),
      h('circle', { cx: 4, cy: 18, r: 1, fill: 'currentColor' }),
    ]),
  },
  grid: {
    draw: () => h('g', { fill: 'none', stroke: 'currentColor', 'stroke-width': 1.8 }, [
      h('rect', { x: 4, y: 4, width: 7, height: 7, rx: 1.5 }),
      h('rect', { x: 13, y: 4, width: 7, height: 7, rx: 1.5 }),
      h('rect', { x: 4, y: 13, width: 7, height: 7, rx: 1.5 }),
      h('rect', { x: 13, y: 13, width: 7, height: 7, rx: 1.5 }),
    ]),
  },
  info: {
    draw: () => h('g', { fill: 'none', stroke: 'currentColor', 'stroke-width': 1.8, 'stroke-linecap': 'round' }, [
      h('circle', { cx: 12, cy: 12, r: 9 }),
      h('path', { d: 'M12 11v5m0-8.5v.01' }),
    ]),
  },
  close: {
    draw: () => h('path', {
      d: 'm6 6 12 12M18 6 6 18',
      stroke: 'currentColor',
      'stroke-width': 2,
      'stroke-linecap': 'round',
    }),
  },
  switch: {
    draw: () => h('path', {
      d: 'M4 8h12l-3-3m3 3-3 3M20 16H8l3 3m-3-3 3-3',
      fill: 'none',
      stroke: 'currentColor',
      'stroke-width': 1.8,
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
    }),
  },
  share: {
    draw: () => h('g', { fill: 'none', stroke: 'currentColor', 'stroke-width': 1.8, 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }, [
      h('circle', { cx: 18, cy: 5, r: 3 }),
      h('circle', { cx: 6, cy: 12, r: 3 }),
      h('circle', { cx: 18, cy: 19, r: 3 }),
      h('path', { d: 'm8.6 13.5 6.8 4M15.4 6.5l-6.8 4' }),
    ]),
  },
  arrowLeft: {
    draw: () => h('path', {
      d: 'M19 12H5m0 0 5-5m-5 5 5 5',
      fill: 'none',
      stroke: 'currentColor',
      'stroke-width': 1.8,
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
    }),
  },
  sortAsc: {
    draw: () => h('path', {
      d: 'M7 4v16m0 0-3-3m3 3 3-3M13 7h7M13 12h5M13 17h3',
      fill: 'none',
      stroke: 'currentColor',
      'stroke-width': 1.8,
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
    }),
  },
  sortDesc: {
    draw: () => h('path', {
      d: 'M7 4v16m0 0 3-3m-3 3-3-3M13 7h3M13 12h5M13 17h7',
      fill: 'none',
      stroke: 'currentColor',
      'stroke-width': 1.8,
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
    }),
  },
  chevronUp: {
    draw: () => h('path', {
      d: 'm6 14 6-6 6 6',
      fill: 'none',
      stroke: 'currentColor',
      'stroke-width': 2,
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
    }),
  },
  chevronDown: {
    draw: () => h('path', {
      d: 'm6 10 6 6 6-6',
      fill: 'none',
      stroke: 'currentColor',
      'stroke-width': 2,
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
    }),
  },
  clock: {
    draw: () => h('g', { fill: 'none', stroke: 'currentColor', 'stroke-width': 1.8, 'stroke-linecap': 'round' }, [
      h('circle', { cx: 12, cy: 12, r: 8 }),
      h('path', { d: 'M12 8v4l3 2' }),
    ]),
  },
};

export const Icon = defineComponent({
  name: 'ShareIcon',
  props: {
    name: { type: String, required: true },
  },
  setup(props) {
    return () => {
      const entry = ICONS[props.name];
      if (!entry) {
        return null;
      }
      return h('svg', {
        viewBox: entry.viewBox ?? '0 0 24 24',
        'aria-hidden': 'true',
      }, [entry.draw()]);
    };
  },
});

export type IconName = keyof typeof ICONS;

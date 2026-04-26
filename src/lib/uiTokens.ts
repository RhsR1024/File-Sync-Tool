/**
 * Design tokens shared across the polish modules.
 *
 * The values are Tailwind class fragments — components compose them with
 * `clsx` / `tailwind-merge` or interpolate them directly.  Keeping them as
 * plain `string` constants means there is zero runtime cost; the bundler
 * tree-shakes whatever a page does not import.
 *
 * These tokens are guidance, not enforcement.  Pages should cite a token
 * whenever the design choice is non-obvious, and reach for raw Tailwind
 * classes when a one-off value is genuinely needed.
 */

export const RADIUS = {
  card: 'rounded-2xl',
  hero: 'rounded-[24px]',
  button: 'rounded-xl',
  pill: 'rounded-full',
  input: 'rounded-lg',
} as const;

export const SHADOW = {
  resting: 'shadow-sm',
  card: 'shadow-[0_14px_40px_rgba(15,23,42,0.06)]',
  hero: 'shadow-[0_18px_60px_rgba(15,23,42,0.08)]',
  modal: 'shadow-[0_20px_70px_rgba(15,23,42,0.18)]',
} as const;

export const SURFACE = {
  page: 'bg-slate-50',
  card: 'bg-white/85 backdrop-blur',
  cardOpaque: 'bg-white',
  inset: 'bg-slate-100',
  sidebar: 'bg-[#0b1220]',
  terminal: 'bg-[#0f172a]',
} as const;

export const TEXT = {
  heading: 'text-slate-950 font-semibold',
  body: 'text-slate-700',
  muted: 'text-slate-500',
  caption: 'text-slate-400 text-xs',
  inverted: 'text-white',
} as const;

export const FOCUS_RING =
  'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500/50 focus-visible:ring-offset-2 focus-visible:ring-offset-white';

export const TRANSITION = {
  default: 'transition-colors duration-150',
  motion: 'transition-all duration-200 ease-out',
  modal: 'transition-opacity duration-200',
} as const;

export const ICON_SIZE = {
  inline: 'h-3.5 w-3.5',
  sm: 'h-4 w-4',
  md: 'h-5 w-5',
  lg: 'h-6 w-6',
} as const;

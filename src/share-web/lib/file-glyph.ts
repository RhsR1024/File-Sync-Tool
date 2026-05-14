export interface FileGlyphStyle {
  label: string;
  color: string;
  bg: string;
  border: string;
}

interface ExtPalette {
  hue: number;
  label: string;
}

const EXT_PALETTE: Record<string, ExtPalette> = {
  pdf:  { hue: 25,  label: 'PDF' },
  doc:  { hue: 240, label: 'DOC' },
  docx: { hue: 240, label: 'DOC' },
  xls:  { hue: 150, label: 'XLS' },
  xlsx: { hue: 150, label: 'XLS' },
  csv:  { hue: 150, label: 'CSV' },
  ppt:  { hue: 30,  label: 'PPT' },
  pptx: { hue: 30,  label: 'PPT' },
  zip:  { hue: 290, label: 'ZIP' },
  rar:  { hue: 290, label: 'RAR' },
  '7z': { hue: 290, label: '7Z' },
  tar:  { hue: 290, label: 'TAR' },
  gz:   { hue: 290, label: 'GZ' },
  bz2:  { hue: 290, label: 'BZ2' },
  xz:   { hue: 290, label: 'XZ' },
  txt:  { hue: 240, label: 'TXT' },
  log:  { hue: 240, label: 'LOG' },
  md:   { hue: 240, label: 'MD' },
  json: { hue: 180, label: 'JSON' },
  xml:  { hue: 180, label: 'XML' },
  yml:  { hue: 180, label: 'YML' },
  yaml: { hue: 180, label: 'YML' },
  toml: { hue: 180, label: 'TOML' },
  ini:  { hue: 180, label: 'INI' },
  conf: { hue: 180, label: 'CONF' },
  sql:  { hue: 180, label: 'SQL' },
  html: { hue: 35,  label: 'HTML' },
  css:  { hue: 240, label: 'CSS' },
  js:   { hue: 80,  label: 'JS' },
  ts:   { hue: 240, label: 'TS' },
  vue:  { hue: 155, label: 'VUE' },
  py:   { hue: 240, label: 'PY' },
  rs:   { hue: 25,  label: 'RS' },
  go:   { hue: 195, label: 'GO' },
  java: { hue: 25,  label: 'JAVA' },
  jar:  { hue: 25,  label: 'JAR' },
  exe:  { hue: 350, label: 'EXE' },
  msi:  { hue: 350, label: 'MSI' },
  bat:  { hue: 350, label: 'BAT' },
  sh:   { hue: 350, label: 'SH' },
  jpg:  { hue: 210, label: 'JPG' },
  jpeg: { hue: 210, label: 'JPG' },
  png:  { hue: 210, label: 'PNG' },
  gif:  { hue: 210, label: 'GIF' },
  svg:  { hue: 210, label: 'SVG' },
  webp: { hue: 210, label: 'WEBP' },
  bmp:  { hue: 210, label: 'BMP' },
  ico:  { hue: 210, label: 'ICO' },
  mp4:  { hue: 290, label: 'MP4' },
  mov:  { hue: 290, label: 'MOV' },
  mkv:  { hue: 290, label: 'MKV' },
  avi:  { hue: 290, label: 'AVI' },
  mp3:  { hue: 350, label: 'MP3' },
  wav:  { hue: 350, label: 'WAV' },
  flac: { hue: 350, label: 'FLAC' },
  iso:  { hue: 240, label: 'ISO' },
  dmg:  { hue: 240, label: 'DMG' },
};

const FALLBACK_HUE = 240;

function paletteFromHue(label: string, hue: number): FileGlyphStyle {
  return {
    label,
    color: `oklch(0.42 0.10 ${hue})`,
    bg: `oklch(0.96 0.025 ${hue})`,
    border: `oklch(0.88 0.05 ${hue})`,
  };
}

export function getFileGlyphStyle(name: string): FileGlyphStyle {
  const lower = (name || '').toLowerCase();

  const compoundMatch = lower.match(/\.tar\.(gz|bz2|xz)$/);
  if (compoundMatch) {
    const palette = EXT_PALETTE[compoundMatch[1]] ?? { hue: 290, label: 'TGZ' };
    return paletteFromHue('TGZ', palette.hue);
  }

  const dot = lower.lastIndexOf('.');
  if (dot < 0 || dot === lower.length - 1) {
    return paletteFromHue('FILE', FALLBACK_HUE);
  }

  const ext = lower.slice(dot + 1);
  const entry = EXT_PALETTE[ext];
  if (entry) {
    return paletteFromHue(entry.label, entry.hue);
  }

  const fallbackLabel = ext.length <= 4 ? ext.toUpperCase() : ext.slice(0, 4).toUpperCase();
  return paletteFromHue(fallbackLabel, FALLBACK_HUE);
}

const CJK = /[一-龥]/;

export function isCjk(value: string): boolean {
  return CJK.test(value);
}

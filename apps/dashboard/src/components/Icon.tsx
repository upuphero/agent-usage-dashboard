const paths = {
  overview: 'M3 3h7v7H3z M14 3h7v7h-7z M3 14h7v7H3z M14 14h7v7h-7z',
  providers: 'M12 3v5 M5 16v5 M19 16v5 M5 16v-4h14v4 M12 8v4 M9 3h6',
  sessions: 'M4 4h16v13H9l-5 4V4 M8 8h8 M8 12h5',
  settings: 'M4 6h16 M4 12h16 M4 18h16 M8 3v6 M16 9v6 M10 15v6',
  refresh: 'M20 11a8 8 0 0 0-14-5L3 9 M3 3v6h6 M4 13a8 8 0 0 0 14 5l3-3 M21 21v-6h-6',
  filter: 'M4 6h16 M7 12h10 M10 18h4 M8 3v6 M16 9v6 M12 15v6',
  close: 'M6 6l12 12 M6 18L18 6',
  'chart-line': 'M4 4v16h16 M7 14l4-5 4 3 5-7',
  'chart-bars': 'M4 4v16h16 M8 16v-4 M13 16V7 M18 16v-7',
  arrow: 'M5 12h14 M13 6l6 6-6 6',
  clock: 'M12 8v5l3 2 M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18',
  check: 'M5 12l4 4L19 6',
  info: 'M12 11v6 M12 7v.01 M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18',
  download: 'M12 3v12 M7 10l5 5 5-5 M4 16v5h16v-5',
  bolt: 'M13 2L4 14h7l-1 8 10-13h-7l0-7',
  shield: 'M12 3l8 3v6c0 5-8 9-8 9s-8-4-8-9V6l8-3 M8 12l3 3 5-6',
  sun: 'M12 2v2 M12 20v2 M2 12h2 M20 12h2 M5 5l1 1 M18 18l1 1 M5 19l1-1 M18 6l1-1 M12 7a5 5 0 1 0 0 10 5 5 0 0 0 0-10',
  language: 'M4 5h12 M10 3v2 M6 5c0 5 3 8 7 10 M14 5c0 5-3 8-9 11 M14 21l4-10 4 10 M15.5 17h5',
} as const;
export type IconName = keyof typeof paths;
export function Icon({ name, className = '' }: { name: IconName; className?: string }) {
  return <svg className={`icon ${className}`} width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={paths[name]} /></svg>;
}

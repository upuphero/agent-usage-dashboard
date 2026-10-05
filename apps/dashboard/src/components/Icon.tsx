const paths = {
  overview: 'M3 3h7v7H3z M14 3h7v7h-7z M3 14h7v7H3z M14 14h7v7h-7z',
  providers: 'M12 3v5 M5 16v5 M19 16v5 M5 16v-4h14v4 M12 8v4 M9 3h6',
  sessions: 'M4 4h16v13H9l-5 4V4 M8 8h8 M8 12h5',
  settings: 'M4 6h16 M4 12h16 M4 18h16 M8 3v6 M16 9v6 M10 15v6',
  refresh: 'M20 11a8 8 0 0 0-14-5L3 9 M3 3v6h6 M4 13a8 8 0 0 0 14 5l3-3 M21 21v-6h-6',
  arrow: 'M5 12h14 M13 6l6 6-6 6',
  clock: 'M12 8v5l3 2 M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18',
  check: 'M5 12l4 4L19 6',
  info: 'M12 11v6 M12 7v.01 M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18',
  download: 'M12 3v12 M7 10l5 5 5-5 M4 16v5h16v-5',
  bolt: 'M13 2L4 14h7l-1 8 10-13h-7l0-7',
  shield: 'M12 3l8 3v6c0 5-8 9-8 9s-8-4-8-9V6l8-3 M8 12l3 3 5-6',
  sun: 'M12 2v2 M12 20v2 M2 12h2 M20 12h2 M5 5l1 1 M18 18l1 1 M5 19l1-1 M18 6l1-1 M12 7a5 5 0 1 0 0 10 5 5 0 0 0 0-10',
} as const;
export type IconName = keyof typeof paths;
export function Icon({ name, className = '' }: { name: IconName; className?: string }) {
  return <svg className={`icon ${className}`} width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={paths[name]} /></svg>;
}

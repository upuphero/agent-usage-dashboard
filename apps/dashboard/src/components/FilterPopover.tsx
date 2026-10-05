import { useEffect, useId, useRef, useState, type ReactNode } from 'react';
import { Icon } from './Icon';
import { useI18n } from '../i18n/I18nContext';

export function FilterPopover({ children, summary, disabled = false }: { children: ReactNode; summary: string; disabled?: boolean }) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const panelId = useId();
  useEffect(() => {
    if (!open) return;
    root.current?.querySelector<HTMLSelectElement>('select')?.focus();
    const outside = (event: PointerEvent) => { if (event.target instanceof Node && !root.current?.contains(event.target)) setOpen(false); };
    const escape = (event: KeyboardEvent) => { if (event.key === 'Escape') { setOpen(false); trigger.current?.focus(); } };
    document.addEventListener('pointerdown', outside);
    document.addEventListener('keydown', escape);
    return () => { document.removeEventListener('pointerdown', outside); document.removeEventListener('keydown', escape); };
  }, [open]);
  return <div className="filter-control" ref={root}>
    <button className={`filter-toggle ${open ? 'is-open' : ''}`} ref={trigger} aria-label={t('筛选用量')} title={`${t('筛选用量')} · ${summary}`} aria-expanded={open} aria-controls={panelId} disabled={disabled} onClick={() => setOpen(value => !value)}><Icon name="filter" /></button>
    {open && <div className="filter-popover" id={panelId} role="dialog" aria-label={t('用量筛选')}><div className="filter-popover-heading"><strong>{t('筛选用量')}</strong><button className="icon-button" aria-label={t('关闭筛选')} onClick={() => { setOpen(false); trigger.current?.focus(); }}><Icon name="close" /></button></div>{children}</div>}
  </div>;
}

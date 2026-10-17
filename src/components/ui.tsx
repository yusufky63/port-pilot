import { Check, ChevronDown, Copy, Loader2, X } from "lucide-react";
import { useEffect, useId, useLayoutEffect, useRef, useState, type ButtonHTMLAttributes, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../lib/cx";

type Variant = "primary" | "secondary" | "ghost" | "danger" | "dangerSolid";
type Size = "sm" | "md" | "lg" | "icon";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
  loading?: boolean;
  icon?: ReactNode;
}

const VARIANTS: Record<Variant, string> = {
  primary: "btn-primary",
  secondary: "border border-line-strong bg-canvas text-ink hover:bg-surface",
  ghost: "text-ink-secondary hover:text-ink hover:bg-surface-muted",
  danger: "border border-danger text-danger-fg hover:bg-danger-soft",
  dangerSolid: "btn-danger-solid",
};
const SIZES: Record<Size, string> = {
  sm: "h-8 px-3 text-[13px] gap-1.5",
  md: "h-9 px-3.5 text-[14px] gap-2",
  lg: "h-11 px-5 text-[15px] gap-2",
  icon: "h-8 w-8",
};

export function Button({ variant = "secondary", size = "md", loading, icon, className, children, disabled, ...rest }: ButtonProps) {
  return (
    <button
      type="button"
      className={cx(
        "inline-flex shrink-0 items-center justify-center whitespace-nowrap rounded-md font-medium transition-colors transition-fast disabled:pointer-events-none disabled:opacity-50",
        VARIANTS[variant],
        SIZES[size],
        className,
      )}
      disabled={disabled || loading}
      {...rest}
    >
      {loading ? <Loader2 size={16} strokeWidth={1.75} className="spin" aria-hidden /> : icon}
      {children}
    </button>
  );
}

export function Card({ className, children, ticks }: { className?: string; children: ReactNode; ticks?: boolean }) {
  return <section className={cx("rounded-lg border border-line bg-canvas", ticks && "ticks", className)}>{children}</section>;
}

export function CardHeader({ title, hint, actions, eyebrow }: { title: ReactNode; hint?: ReactNode; actions?: ReactNode; eyebrow?: string }) {
  return (
    <header className="flex items-start justify-between gap-4 border-b border-line px-4 py-3">
      <div className="min-w-0">
        {eyebrow && <div className="eyebrow mb-1">{eyebrow}</div>}
        <h2 className="display-medium text-[16px] text-ink">{title}</h2>
        {hint && <p className="mt-0.5 text-[13px] text-ink-muted">{hint}</p>}
      </div>
      {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
    </header>
  );
}

export type Tone = "neutral" | "positive" | "warning" | "danger" | "primary";
const TONES: Record<Tone, string> = {
  neutral: "bg-surface-muted text-ink-secondary",
  positive: "bg-positive-soft text-positive-fg",
  warning: "bg-warning-soft text-warning-fg",
  danger: "bg-danger-soft text-danger-fg",
  primary: "bg-primary-soft text-primary",
};

export function Badge({ tone = "neutral", children, className, title, icon }: { tone?: Tone; children: ReactNode; className?: string; title?: string; icon?: ReactNode }) {
  return (
    <span title={title} className={cx("inline-flex h-5 items-center gap-1 whitespace-nowrap rounded-sm px-1.5 font-mono text-[11px] leading-none", TONES[tone], className)}>
      {icon}
      {children}
    </span>
  );
}

export function Checkbox({ checked, onChange, disabled, label }: { checked: boolean; onChange: (checked: boolean) => void; disabled?: boolean; label: string }) {
  return (
    <label className={cx("inline-flex cursor-pointer items-center gap-2.5 text-[14px] text-ink", disabled && "cursor-default opacity-50")}>
      <button
        type="button"
        role="checkbox"
        aria-checked={checked}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={cx(
          "inline-flex h-[18px] w-[18px] shrink-0 items-center justify-center rounded-[5px] border transition-colors transition-fast",
          checked ? "border-primary bg-primary-fill text-primary-contrast" : "border-line-strong bg-canvas hover:border-primary",
        )}
      >
        {checked && <Check size={12} strokeWidth={3} />}
      </button>
      <span>{label}</span>
    </label>
  );
}

export function Switch({ checked, onChange, disabled, label }: { checked: boolean; onChange: (v: boolean) => void; disabled?: boolean; label: string }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cx(
        "relative inline-flex h-5 w-9 shrink-0 items-center rounded-full border transition-colors transition-fast disabled:opacity-40",
        checked ? "border-primary bg-primary-fill" : "border-line-strong bg-surface-muted",
      )}
    >
      <span className={cx("inline-block h-3.5 w-3.5 rounded-full transition-transform transition-fast", checked ? "translate-x-[18px] bg-primary-contrast" : "translate-x-[2px] bg-ink-muted")} />
    </button>
  );
}

export function Segmented<T extends string>({ value, options, onChange, size = "md", label }: { value: T; options: { value: T; label: ReactNode }[]; onChange: (v: T) => void; size?: "sm" | "md"; label?: string }) {
  return (
    <div role="radiogroup" aria-label={label} className="inline-flex rounded-md border border-line bg-surface p-0.5">
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={o.value === value}
          onClick={() => onChange(o.value)}
          className={cx(
            "inline-flex items-center gap-1.5 whitespace-nowrap rounded-[6px] px-3 font-medium transition-colors transition-fast",
            size === "sm" ? "h-7 text-[12px]" : "h-8 text-[13px]",
            o.value === value ? "border border-line bg-canvas text-ink" : "border border-transparent text-ink-secondary hover:text-ink",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** A toggle chip for multi-select filters. */
export function Chip({ active, onClick, children, count }: { active: boolean; onClick: () => void; children: ReactNode; count?: number }) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      className={cx(
        "inline-flex h-8 items-center gap-1.5 whitespace-nowrap rounded-md border px-2.5 text-[13px] font-medium transition-colors transition-fast",
        active ? "border-primary bg-primary-soft text-ink" : "border-line text-ink-secondary hover:border-line-strong hover:text-ink",
      )}
    >
      {children}
      {count !== undefined && <span className="num font-mono text-[11px] text-ink-muted">{count}</span>}
    </button>
  );
}

/**
 * Dropdown built from buttons instead of a native <select>: the native popup ignores the app
 * theme in WebView2 and shows light lists in dark mode.
 */
export function Select<T extends string | number>({ value, options, onChange, label, className }: { value: T; options: { value: T; label: string }[]; onChange: (v: T) => void; label: string; className?: string }) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [up, setUp] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const listId = useId();
  const current = options.find((o) => o.value === value);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [open]);

  useLayoutEffect(() => {
    if (!open) return;
    const box = rootRef.current?.getBoundingClientRect();
    if (box) setUp(window.innerHeight - box.bottom < 260 && box.top > 260);
  }, [open]);

  const openList = () => {
    setActive(Math.max(0, options.findIndex((o) => o.value === value)));
    setOpen(true);
  };
  const choose = (i: number) => {
    const o = options[i];
    if (o) onChange(o.value);
    setOpen(false);
  };
  const onKey = (e: ReactKeyboardEvent<HTMLButtonElement>) => {
    if (!open && (e.key === "ArrowDown" || e.key === "ArrowUp" || e.key === "Enter" || e.key === " ")) {
      e.preventDefault();
      openList();
      return;
    }
    if (!open) return;
    if (e.key === "Escape" || e.key === "Tab") setOpen(false);
    else if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((a) => Math.min(options.length - 1, a + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((a) => Math.max(0, a - 1));
    } else if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      choose(active);
    }
  };

  return (
    <div ref={rootRef} className={cx("relative inline-flex", className)}>
      <button
        type="button"
        role="combobox"
        aria-label={label}
        aria-expanded={open}
        aria-controls={listId}
        aria-activedescendant={open ? `${listId}-${active}` : undefined}
        onClick={() => (open ? setOpen(false) : openList())}
        onKeyDown={onKey}
        className={cx("field inline-flex w-full min-w-0 items-center justify-between gap-2 text-left hover:border-ink-secondary", open && "border-primary")}
      >
        <span className="truncate">{current?.label ?? ""}</span>
        <ChevronDown size={14} strokeWidth={1.75} className={cx("shrink-0 text-ink-muted transition-transform transition-fast", open && "rotate-180")} aria-hidden />
      </button>
      {open && (
        <ul
          id={listId}
          role="listbox"
          aria-label={label}
          className={cx("z-popover anim-fade absolute left-0 max-h-[260px] min-w-full overflow-y-auto rounded-md border border-line bg-canvas p-1 shadow-popover", up ? "bottom-full mb-1" : "top-full mt-1")}
        >
          {options.map((o, i) => (
            <li
              key={String(o.value)}
              id={`${listId}-${i}`}
              role="option"
              aria-selected={o.value === value}
              onMouseEnter={() => setActive(i)}
              onMouseDown={(e) => {
                e.preventDefault();
                choose(i);
              }}
              className={cx("flex h-8 cursor-pointer items-center justify-between gap-3 whitespace-nowrap rounded-[6px] px-2.5 text-[14px]", i === active ? "bg-surface-muted text-ink" : "text-ink-secondary")}
            >
              {o.label}
              {o.value === value && <Check size={14} strokeWidth={2} className="text-primary" aria-hidden />}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export function EmptyState({ icon, title, body, action, className }: { icon?: ReactNode; title: string; body?: ReactNode; action?: ReactNode; className?: string }) {
  return (
    <div className={cx("flex flex-col items-center justify-center px-6 py-14 text-center", className)}>
      {icon && <div className="mb-4 flex h-11 w-11 items-center justify-center rounded-lg border border-line bg-surface text-primary">{icon}</div>}
      <h3 className="display-medium text-[18px] text-ink">{title}</h3>
      {body && <p className="mt-1.5 max-w-[56ch] text-[14px] text-ink-secondary">{body}</p>}
      {action && <div className="mt-5 flex flex-wrap items-center justify-center gap-2">{action}</div>}
    </div>
  );
}

export function Banner({ tone = "neutral", icon, children, action }: { tone?: Tone; icon?: ReactNode; children: ReactNode; action?: ReactNode }) {
  return (
    <div className={cx("flex items-start gap-3 rounded-md px-3 py-2.5 text-[13px]", TONES[tone])}>
      {icon && <span className="mt-0.5 shrink-0">{icon}</span>}
      <div className="min-w-0 flex-1">{children}</div>
      {action}
    </div>
  );
}

export function Skeleton({ className }: { className?: string }) {
  return <div className={cx("rounded-md bg-surface-muted", className)} />;
}

export function Dialog({ open, onClose, title, children, footer }: { open: boolean; onClose: () => void; title: ReactNode; children: ReactNode; footer?: ReactNode }) {
  const { t } = useTranslation();
  const titleId = useId();
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    ref.current?.querySelector<HTMLElement>("[data-autofocus]")?.focus();
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);
  if (!open) return null;
  return (
    <div className="z-dialog anim-fade fixed inset-0 flex items-center justify-center bg-scrim p-6" onMouseDown={onClose}>
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        onMouseDown={(e) => e.stopPropagation()}
        className="anim-rise flex max-h-full w-full max-w-[540px] flex-col overflow-hidden rounded-xl border border-line bg-canvas shadow-overlay"
      >
        <header className="flex items-start justify-between gap-4 border-b border-line px-5 py-4">
          <h2 id={titleId} className="display-medium text-[18px] text-ink">
            {title}
          </h2>
          <button type="button" aria-label={t("common.close")} title={t("common.close")} onClick={onClose} className="-mr-1 rounded-md p-1 text-ink-muted hover:bg-surface-muted hover:text-ink">
            <X size={18} strokeWidth={1.75} />
          </button>
        </header>
        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">{children}</div>
        {footer && <footer className="flex items-center justify-end gap-2 border-t border-line bg-surface px-5 py-3">{footer}</footer>}
      </div>
    </div>
  );
}

export function PageHeader({ title, subtitle, actions, meta }: { title: string; subtitle?: string; actions?: ReactNode; meta?: ReactNode }) {
  return (
    <div className="mb-5 flex flex-wrap items-end justify-between gap-4">
      <div className="min-w-0">
        <h1 className="display text-[24px] leading-tight text-ink">{title}</h1>
        {subtitle && <p className="mt-1 max-w-[72ch] text-[14px] text-ink-secondary">{subtitle}</p>}
        {meta && <div className="mt-2 flex flex-wrap items-center gap-2 text-[12px] text-ink-muted">{meta}</div>}
      </div>
      {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
    </div>
  );
}

export function Stat({ label, value, sub, className }: { label: ReactNode; value: ReactNode; sub?: ReactNode; className?: string }) {
  return (
    <div className={cx("px-4 py-3.5", className)}>
      <div className="text-[13px] text-ink-secondary">{label}</div>
      <div className="num mt-1 font-mono text-[24px] font-medium leading-none tracking-tight text-ink">{value}</div>
      {sub && <div className="mt-1.5 truncate font-mono text-[12px] text-ink-muted">{sub}</div>}
    </div>
  );
}

export function SectionTitle({ children, aside }: { children: ReactNode; aside?: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-3 bg-surface px-4 py-2">
      <span className="label">{children}</span>
      {aside && <span className="font-mono text-[12px] text-ink-muted">{aside}</span>}
    </div>
  );
}

/** A label/value row for detail views; values are selectable. */
export function Field({ label, children, mono }: { label: ReactNode; children: ReactNode; mono?: boolean }) {
  return (
    <div className="grid grid-cols-[112px_minmax(0,1fr)] gap-3 py-1.5 text-[13px]">
      <dt className="text-ink-muted">{label}</dt>
      <dd className={cx("selectable min-w-0 break-words text-ink", mono && "font-mono text-[12px]")}>{children}</dd>
    </div>
  );
}

export function CopyButton({ text, label }: { text: string; label?: string }) {
  const { t } = useTranslation();
  const [done, setDone] = useState(false);
  useEffect(() => {
    if (!done) return;
    const id = setTimeout(() => setDone(false), 1500);
    return () => clearTimeout(id);
  }, [done]);
  return (
    <button
      type="button"
      onClick={() => {
        void navigator.clipboard.writeText(text).then(() => setDone(true));
      }}
      title={done ? t("common.copied") : (label ?? t("common.copy"))}
      aria-label={label ?? t("common.copy")}
      className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-ink-muted hover:bg-surface-muted hover:text-ink"
    >
      {done ? <Check size={13} strokeWidth={2} className="text-positive-fg" /> : <Copy size={13} strokeWidth={1.75} />}
    </button>
  );
}

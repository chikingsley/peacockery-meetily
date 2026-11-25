import type { ReactNode } from "react";

export function agendaTime(value: string) {
  return new Date(value).toLocaleTimeString("en-US", {
    timeZone: "America/Phoenix",
    hour: "numeric",
    minute: "2-digit",
  });
}

export function agendaDay(value: string | number) {
  return new Date(value).toLocaleDateString("en-US", {
    timeZone: "America/Phoenix",
    weekday: "long",
    month: "long",
    day: "numeric",
  });
}

export function AgendaRow({
  time,
  title,
  detail,
  status,
  actions,
  onSelect,
  selected,
  muted,
}: {
  time: string;
  title: string;
  detail?: string;
  status?: string;
  actions?: ReactNode;
  onSelect?: () => void;
  selected?: boolean;
  muted?: boolean;
}) {
  const content = (
    <>
      <span className="text-sm font-medium whitespace-nowrap tabular-nums">
        {time}
      </span>
      <span className="min-w-0">
        <span className="block text-sm font-medium">{title}</span>
        {detail && (
          <span className="block text-xs text-muted-foreground">{detail}</span>
        )}
        {status && <span className="block text-xs font-medium">{status}</span>}
      </span>
    </>
  );

  const layout =
    "grid min-w-0 flex-1 grid-cols-[4.75rem_minmax(0,1fr)] items-baseline gap-2 px-3 py-2 text-left";

  return (
    <div
      className={`flex items-center rounded-md border ${selected ? "border-primary bg-accent" : ""} ${muted ? "text-muted-foreground" : ""}`}
    >
      {onSelect ? (
        <button
          type="button"
          aria-pressed={selected}
          onClick={onSelect}
          className={`${layout} rounded-md hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring`}
        >
          {content}
        </button>
      ) : (
        <div className={layout}>{content}</div>
      )}
      {actions && (
        <div className="flex shrink-0 items-center gap-1 pr-2">{actions}</div>
      )}
    </div>
  );
}

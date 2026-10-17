import { useTranslation } from "react-i18next";
import { cx } from "../lib/cx";
import { useAge } from "../lib/hooks";
import type { Listener } from "../lib/types";
import { ExposureBadge, LauncherLabel, OwnerLabel, ProjectLabel, RuntimeLabel } from "./labels";
import { Badge } from "./ui";

export function ListenerTable({ listeners, selectedId, onSelect }: { listeners: Listener[]; selectedId: string | null; onSelect: (id: string) => void }) {
  const { t } = useTranslation();
  const age = useAge();
  return (
    <div className="@container overflow-x-auto">
      <table className="w-full table-fixed border-collapse text-left">
        <colgroup>
          <col className="w-[104px]" />
          <col />
          <col />
          <col className="hidden w-[136px] @2xl:table-column" />
          <col className="w-[156px]" />
          <col className="hidden w-[112px] @5xl:table-column" />
          <col className="hidden w-[132px] @4xl:table-column" />
        </colgroup>
        <thead className="z-sticky sticky top-0 bg-surface">
          <tr className="label border-b border-line">
            <th className="px-4 py-2 font-normal">{t("ports.colPort")}</th>
            <th className="px-3 py-2 font-normal">{t("ports.colOwner")}</th>
            <th className="px-3 py-2 font-normal">{t("ports.colProject")}</th>
            <th className="hidden px-3 py-2 font-normal @2xl:table-cell">{t("ports.colRuntime")}</th>
            <th className="px-3 py-2 font-normal">{t("ports.colExposure")}</th>
            <th className="hidden px-3 py-2 font-normal @5xl:table-cell">{t("ports.colStarted")}</th>
            <th className="hidden px-3 py-2 font-normal @4xl:table-cell">{t("ports.colStartedBy")}</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-line">
          {listeners.map((l) => {
            const selected = l.id === selectedId;
            return (
              <tr
                key={l.id}
                tabIndex={0}
                aria-selected={selected}
                onClick={() => onSelect(l.id)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    onSelect(l.id);
                  }
                }}
                className={cx("cursor-pointer align-middle transition-colors transition-fast focus-visible:outline-offset-[-2px]", selected ? "bg-primary-soft" : "hover:bg-surface")}
              >
                <td className={cx("border-l-2 px-4 py-2.5", selected ? "border-primary" : "border-transparent")}>
                  <div className="flex items-baseline gap-1.5">
                    <span className="num font-mono text-[15px] font-medium text-ink">{l.port}</span>
                    {l.protocol === "udp" && <span className="font-mono text-[10px] uppercase text-ink-muted">udp</span>}
                  </div>
                  {l.conflict && (
                    <Badge tone="danger" className="mt-1">
                      {t("ports.conflict")}
                    </Badge>
                  )}
                </td>
                <td className="px-3 py-2.5">
                  <OwnerLabel listener={l} />
                </td>
                <td className="px-3 py-2.5">
                  <ProjectLabel project={l.project} />
                </td>
                <td className="hidden px-3 py-2.5 @2xl:table-cell">
                  <RuntimeLabel listener={l} />
                </td>
                <td className="px-3 py-2.5">
                  <ExposureBadge exposure={l.exposure} />
                </td>
                <td className="hidden px-3 py-2.5 font-mono text-[12px] text-ink-muted @5xl:table-cell">{age(l.process?.startedAt ?? null) || "-"}</td>
                <td className="hidden px-3 py-2.5 @4xl:table-cell">
                  <LauncherLabel launcher={l.launcher} />
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

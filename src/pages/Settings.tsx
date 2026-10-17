import { AlertTriangle, ShieldCheck } from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Banner, Card, CardHeader, PageHeader, Segmented, Select, Switch } from "../components/ui";
import { LANGUAGES } from "../i18n/languages";
import type { Settings } from "../lib/types";
import { useApp } from "../store/app";

function Row({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-6 px-4 py-3">
      <div className="min-w-0">
        <div className="text-[14px] text-ink">{label}</div>
        {hint && <div className="mt-0.5 max-w-[60ch] text-[13px] text-ink-muted">{hint}</div>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

const REFRESH = [2, 3, 5, 10, 30];
const GRACE = [3, 5, 10, 15, 30];

export function SettingsPage() {
  const { t } = useTranslation();
  const settings = useApp((s) => s.settings);
  const info = useApp((s) => s.info);
  const save = useApp((s) => s.saveSettings);
  if (!settings) return null;
  const set = (patch: Partial<Settings>) => void save(patch);

  return (
    <>
      <PageHeader title={t("settings.title")} />
      {info?.settingsError && (
        <div className="mb-4">
          <Banner tone="danger" icon={<AlertTriangle size={16} strokeWidth={1.75} />}>
            {t("settings.readError")}
          </Banner>
        </div>
      )}
      <div className="grid gap-4 xl:grid-cols-2">
        <div className="space-y-4">
          <Card>
            <CardHeader title={t("settings.appearance")} />
            <div className="divide-y divide-line">
              <Row label={t("settings.theme")}>
                <Segmented<Settings["theme"]>
                  value={settings.theme}
                  onChange={(theme) => set({ theme })}
                  options={[
                    { value: "system", label: t("settings.themeSystem") },
                    { value: "light", label: t("settings.themeLight") },
                    { value: "dark", label: t("settings.themeDark") },
                  ]}
                />
              </Row>
              <Row label={t("settings.language")}>
                <Select<Settings["language"]>
                  label={t("settings.language")}
                  value={settings.language}
                  onChange={(language) => set({ language })}
                  className="w-[160px]"
                  options={[{ value: "system", label: t("settings.languageSystem") }, ...LANGUAGES.map((l) => ({ value: l.code, label: l.name }))]}
                />
              </Row>
            </div>
          </Card>

          <Card>
            <CardHeader title={t("settings.discovery")} />
            <div className="divide-y divide-line">
              <Row label={t("settings.refresh")}>
                <Select<number>
                  label={t("settings.refresh")}
                  value={settings.refreshSeconds}
                  onChange={(refreshSeconds) => set({ refreshSeconds })}
                  className="w-[110px]"
                  options={REFRESH.map((s) => ({ value: s, label: t("settings.seconds", { count: s }) }))}
                />
              </Row>
              <Row label={t("settings.wsl")} hint={t("settings.wslHint")}>
                <Switch checked={settings.includeWsl} onChange={(includeWsl) => set({ includeWsl })} label={t("settings.wsl")} />
              </Row>
              <Row label={t("settings.docker")} hint={t("settings.dockerHint")}>
                <Switch checked={settings.includeDocker} onChange={(includeDocker) => set({ includeDocker })} label={t("settings.docker")} />
              </Row>
              <Row label={t("settings.mask")} hint={t("settings.maskHint")}>
                <Switch checked={settings.maskSecrets} onChange={(maskSecrets) => set({ maskSecrets })} label={t("settings.mask")} />
              </Row>
            </div>
          </Card>
        </div>

        <div className="space-y-4">
          <Card>
            <CardHeader title={t("settings.stopping")} />
            <div className="divide-y divide-line">
              <Row label={t("settings.grace")} hint={t("settings.graceHint")}>
                <Select<number>
                  label={t("settings.grace")}
                  value={settings.graceSeconds}
                  onChange={(graceSeconds) => set({ graceSeconds })}
                  className="w-[110px]"
                  options={GRACE.map((s) => ({ value: s, label: t("settings.seconds", { count: s }) }))}
                />
              </Row>
            </div>
          </Card>

          <Card>
            <CardHeader title={t("settings.window")} />
            <div className="divide-y divide-line">
              <Row label={t("settings.closeToTray")}>
                <Switch checked={settings.closeToTray} onChange={(closeToTray) => set({ closeToTray })} label={t("settings.closeToTray")} />
              </Row>
            </div>
          </Card>

          <Card ticks>
            <CardHeader title={t("settings.about")} />
            <dl className="space-y-2 px-4 py-3 text-[13px]">
              <div className="flex justify-between gap-4">
                <dt className="text-ink-muted">{t("settings.version")}</dt>
                <dd className="selectable font-mono text-ink">{info?.version}</dd>
              </div>
              <div className="flex justify-between gap-4">
                <dt className="shrink-0 text-ink-muted">{t("settings.dataDir")}</dt>
                <dd className="selectable min-w-0 truncate font-mono text-[12px] text-ink" title={info?.dataDir}>
                  {info?.dataDir}
                </dd>
              </div>
              <div className="flex justify-between gap-4">
                <dt className="text-ink-muted">{t("settings.license")}</dt>
                <dd className="font-mono text-ink">MIT OR Apache-2.0</dd>
              </div>
              {info?.portable && <p className="text-ink-secondary">{t("settings.portable")}</p>}
            </dl>
            <div className="border-t border-line px-4 py-3">
              <Banner tone="positive" icon={<ShieldCheck size={16} strokeWidth={1.75} />}>
                {t("settings.privacy")}
              </Banner>
            </div>
          </Card>
        </div>
      </div>
    </>
  );
}

import { AlertTriangle, ShieldCheck } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Banner, Button, Card, CardHeader, Dialog, PageHeader, Segmented, Select, Switch } from "../components/ui";
import { LANGUAGES } from "../i18n/languages";
import { api } from "../lib/api";
import { localDateTime } from "../lib/format";
import { useLocale } from "../lib/hooks";
import type { Settings, TrustEntry } from "../lib/types";
import { errorText, useApp } from "../store/app";

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

function UpdatesCard({ settings, set }: { settings: Settings; set: (patch: Partial<Settings>) => void }) {
  const { t } = useTranslation();
  const info = useApp((s) => s.info);
  const update = useApp((s) => s.update);
  const setUpdate = useApp((s) => s.setUpdate);
  const dismissUpdate = useApp((s) => s.dismissUpdate);
  const [state, setState] = useState<"idle" | "checking" | "done">("idle");
  const [error, setError] = useState<string | null>(null);

  const check = () => {
    setState("checking");
    setError(null);
    api
      .updateCheck()
      .then((u) => {
        setUpdate(u);
        // A manual check brings back a banner the user set aside.
        dismissUpdate(null);
      })
      .catch((e) => setError(errorText(e)))
      .finally(() => setState("done"));
  };

  return (
    <Card>
      <CardHeader
        title={t("settings.updates")}
        actions={
          <Button size="sm" onClick={check} loading={state === "checking"}>
            {state === "checking" ? t("settings.checking") : t("settings.checkNow")}
          </Button>
        }
      />
      <div className="divide-y divide-line">
        <Row label={t("settings.autoUpdate")} hint={t("settings.autoUpdateHint")}>
          <Switch checked={settings.autoUpdate} onChange={(autoUpdate) => set({ autoUpdate })} label={t("settings.autoUpdate")} />
        </Row>
        {(state === "done" || update) && (
          <div className="flex items-center justify-between gap-4 px-4 py-3 text-[13px]">
            <span className={error ? "text-danger-fg" : "text-ink-secondary"}>
              {error
                ? t("toast.failed", { error })
                : update
                  ? t("settings.updateFound", { version: update.version })
                  : t("settings.upToDate", { version: info?.version ?? "" })}
            </span>
            <Button size="sm" variant="ghost" onClick={() => void api.openReleasePage()}>
              {t("settings.releasePage")}
            </Button>
          </div>
        )}
      </div>
    </Card>
  );
}

function HistoryCard({ settings, set }: { settings: Settings; set: (patch: Partial<Settings>) => void }) {
  const { t } = useTranslation();
  const toast = useApp((s) => s.toast);
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  const clear = () => {
    setBusy(true);
    api
      .historyClear()
      .then(() => toast("positive", t("settings.historyCleared")))
      .catch((e) => toast("danger", t("toast.failed", { error: errorText(e) })))
      .finally(() => {
        setBusy(false);
        setConfirm(false);
      });
  };
  return (
    <Card>
      <CardHeader
        title={t("settings.history")}
        actions={
          <Button size="sm" variant="danger" onClick={() => setConfirm(true)}>
            {t("settings.clearHistory")}
          </Button>
        }
      />
      <div className="divide-y divide-line">
        <Row label={t("settings.historyOn")} hint={t("settings.historyOnHint")}>
          <Switch checked={settings.history} onChange={(history) => set({ history })} label={t("settings.historyOn")} />
        </Row>
        <Row label={t("settings.historyCommands")} hint={t("settings.historyCommandsHint")}>
          <Switch checked={settings.historyCommandLines} disabled={!settings.history} onChange={(historyCommandLines) => set({ historyCommandLines })} label={t("settings.historyCommands")} />
        </Row>
      </div>
      <Dialog
        open={confirm}
        onClose={() => setConfirm(false)}
        title={t("settings.clearTitle")}
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirm(false)} data-autofocus>
              {t("common.cancel")}
            </Button>
            <Button variant="dangerSolid" loading={busy} onClick={clear}>
              {t("settings.clearConfirm")}
            </Button>
          </>
        }
      >
        <p className="text-[14px] text-ink-secondary">{t("settings.clearBody")}</p>
      </Dialog>
    </Card>
  );
}

function TrustedCard() {
  const { t } = useTranslation();
  const locale = useLocale();
  const toast = useApp((s) => s.toast);
  const [list, setList] = useState<TrustEntry[] | null>(null);
  const [version, setVersion] = useState(0);
  useEffect(() => {
    let alive = true;
    api
      .trusted()
      .then((l) => alive && setList(l))
      .catch(() => alive && setList([]));
    return () => {
      alive = false;
    };
  }, [version]);
  const revoke = (e: TrustEntry) => {
    api
      .trustRevoke(e.projectKey, e.run)
      .then(() => {
        toast("positive", t("settings.revoked", { run: e.run }));
        setVersion((v) => v + 1);
      })
      .catch((err) => toast("danger", t("toast.failed", { error: errorText(err) })));
  };
  return (
    <Card>
      <CardHeader title={t("settings.trusted")} hint={t("settings.trustedHint")} />
      {list !== null && list.length === 0 ? (
        <p className="px-4 py-3 text-[13px] text-ink-muted">{t("settings.trustedEmpty")}</p>
      ) : (
        <ul className="divide-y divide-line">
          {(list ?? []).map((e) => (
            <li key={`${e.projectKey}|${e.run}`} className="flex items-center justify-between gap-4 px-4 py-2.5">
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <span className="font-mono text-[13px] text-ink">{e.run}</span>
                  <span className="truncate text-[13px] text-ink-secondary">{e.project}</span>
                </div>
                <div className="truncate font-mono text-[11px] text-ink-muted" title={e.folder}>
                  {e.folder} · {t("settings.trustedOn", { date: localDateTime(new Date(e.trustedAt).toISOString(), locale) })}
                </div>
              </div>
              <Button size="sm" variant="ghost" onClick={() => revoke(e)}>
                {t("settings.revoke")}
              </Button>
            </li>
          ))}
        </ul>
      )}
    </Card>
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
      {info?.storeError && (
        <div className="mb-4">
          <Banner tone="danger" icon={<AlertTriangle size={16} strokeWidth={1.75} />}>
            {t("settings.storeError", { error: info.storeError })}
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

          <UpdatesCard settings={settings} set={set} />

          <HistoryCard settings={settings} set={set} />

          <TrustedCard />

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

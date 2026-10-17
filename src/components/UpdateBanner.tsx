import { Download } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../lib/api";
import { errorText, useApp } from "../store/app";
import { Banner, Button } from "./ui";

/** Shown above every page while a newer release is known and the user has not set it aside. */
export function UpdateBanner() {
  const { t } = useTranslation();
  const update = useApp((s) => s.update);
  const dismissed = useApp((s) => s.updateDismissed);
  const dismiss = useApp((s) => s.dismissUpdate);
  const toast = useApp((s) => s.toast);
  const activeRuns = useApp((s) => s.runs.filter((r) => r.state !== "exited").length);
  const [busy, setBusy] = useState(false);
  if (!update || dismissed === update.version) return null;

  const install = () => {
    setBusy(true);
    // On success the installer takes over and the app restarts; nothing comes back.
    api
      .updateInstall()
      .catch((e) => toast("danger", t("toast.failed", { error: errorText(e) })))
      .finally(() => setBusy(false));
  };

  return (
    <div className="mb-4">
      <Banner
        tone="primary"
        icon={<Download size={16} strokeWidth={1.75} />}
        action={
          <span className="flex shrink-0 items-center gap-2">
            <Button size="sm" variant="ghost" onClick={() => dismiss(update.version)} disabled={busy}>
              {t("update.later")}
            </Button>
            <Button size="sm" variant="primary" onClick={install} loading={busy}>
              {busy ? t("update.installing") : update.portable ? t("update.download") : t("update.install")}
            </Button>
          </span>
        }
      >
        <div className="font-medium text-ink">{t("update.available", { version: update.version, current: update.current })}</div>
        <div className="mt-0.5 text-ink-secondary">
          {update.portable ? t("update.portableHint") : activeRuns > 0 ? t("update.runsNote", { count: activeRuns }) : t("update.restartNote")}
        </div>
      </Banner>
    </div>
  );
}

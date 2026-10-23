import { formatRunTime, triggerLabel } from "../format";
import { useI18n } from "../i18n";
import type { StatusView } from "../types";

export default function StatusLine({ status }: { status: StatusView }) {
  const { lang, t } = useI18n();
  const last = status.last_success;
  const next = status.next_run;
  return (
    <div className="status-line">
      <span>
        {t("statusLine.lastSuccess")}
        {last ? (
          <>
            {formatRunTime(last.created_at, lang)}
            {" · "}
            {triggerLabel(t, last.trigger)}
            {last.overall_status === "partial" && t("statusLine.partial")}
            {last.destinations.length === 0 && t("statusLine.unchanged")}
          </>
        ) : (
          t("statusLine.none")
        )}
      </span>
      <span>
        {t("statusLine.nextRun")}
        {!next
          ? t("statusLine.off")
          : next.overdue
            ? t("statusLine.overdue")
            : next.at}
      </span>
    </div>
  );
}

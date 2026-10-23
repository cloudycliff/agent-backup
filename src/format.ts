import type { Lang, MessageKey, Translate } from "./i18n";

const TRIGGER_KEYS: Record<string, MessageKey> = {
  manual: "trigger.manual",
  schedule: "trigger.schedule",
  retry: "trigger.retry",
};

const STATUS_KEYS: Record<string, MessageKey> = {
  ok: "status.ok",
  partial: "status.partial",
  failed: "status.failed",
  skipped: "status.skipped",
};

export const triggerLabel = (t: Translate, trigger: string) =>
  trigger in TRIGGER_KEYS ? t(TRIGGER_KEYS[trigger]) : trigger;

export const statusLabel = (t: Translate, status: string) =>
  status in STATUS_KEYS ? t(STATUS_KEYS[status]) : status;

export function formatBytes(n: number) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

/** Run stamps are UTC `YYYY-MM-DDTHHMMSSZ`; show them in local time. */
export function formatRunTime(stamp: string, lang: Lang) {
  const m = /^(\d{4})-(\d{2})-(\d{2})T(\d{2})(\d{2})(\d{2})Z$/.exec(stamp);
  if (!m) return stamp;
  const [, y, mo, d, h, mi, s] = m.map(Number);
  return new Date(Date.UTC(y, mo - 1, d, h, mi, s)).toLocaleString(
    lang === "zh" ? "zh-CN" : "en-US",
    {
      hour12: false,
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
    },
  );
}

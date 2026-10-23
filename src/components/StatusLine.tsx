import { formatRunTime, TRIGGER_LABELS } from "../format";
import type { StatusView } from "../types";

export default function StatusLine({ status }: { status: StatusView }) {
  const last = status.last_success;
  const next = status.next_run;
  return (
    <div className="status-line">
      <span>
        上次成功备份：
        {last ? (
          <>
            {formatRunTime(last.created_at)}
            {" · "}
            {TRIGGER_LABELS[last.trigger] ?? last.trigger}
            {last.overall_status === "partial" && "（部分成功）"}
            {last.destinations.length === 0 && "（内容无变化）"}
          </>
        ) : (
          "暂无"
        )}
      </span>
      <span>
        下次定时：
        {!next ? "未开启" : next.overdue ? "已错过，即将补跑" : next.at}
      </span>
    </div>
  );
}

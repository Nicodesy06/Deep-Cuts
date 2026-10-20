// Sections that are on the roadmap but not built yet. They say so plainly:
// no mock data, no buttons that do nothing.
import type { MsgKey } from "../i18n";
import { useApp, type View } from "../store";

const INFO: Partial<Record<View, { title: MsgKey; body: MsgKey; phase: string; available?: MsgKey }>> = {
  discover: { title: "nav.discover", body: "planned.discover", phase: "5", available: "planned.discoverNow" },
  dna: { title: "nav.dna", body: "planned.dna", phase: "4", available: "planned.dnaNow" },
  sets: { title: "nav.sets", body: "planned.sets", phase: "6", available: "planned.setsNow" },
  tags: { title: "nav.tags", body: "planned.tags", phase: "2", available: "planned.tagsNow" },
};

export function Planned({ view }: { view: View }) {
  const { t } = useApp();
  const info = INFO[view];
  if (!info) return null;
  return (
    <div className="page">
      <header className="page-head">
        <h1>{t(info.title)}</h1>
        <span className="badge">{t("planned.badge", { phase: info.phase })}</span>
      </header>
      <section className="panel planned">
        <p>{t(info.body)}</p>
        {info.available && (
          <>
            <h2>{t("planned.todayTitle")}</h2>
            <p className="muted">{t(info.available)}</p>
          </>
        )}
        <p className="hint">{t("planned.honesty")}</p>
      </section>
    </div>
  );
}

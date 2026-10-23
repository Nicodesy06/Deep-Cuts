// One-click presets for a style of set. A profile only touches settings that
// change something measurable: the BPM range used to settle half/double tempo,
// how much agreement genre suggestions need, the Set Builder's default order
// and the weights of Similar tracks. Each card lists exactly what it changes,
// and the previous values can be restored.
import { useState } from "react";
import { api, DEFAULT_WEIGHTS, type SimilarityWeights } from "../api";
import type { MsgKey } from "../i18n";
import { useAction, useApp } from "../store";

export interface DjProfile {
  id: string;
  bpmRange: string;
  genreMinAgreement: number;
  setMode: "smooth" | "build" | "arc";
  /** Changes to the default Similar-tracks weights. */
  weights: Partial<SimilarityWeights>;
}

export const DJ_PROFILES: DjProfile[] = [
  { id: "house", bpmRange: "78-155", genreMinAgreement: 0.6, setMode: "build", weights: {} },
  { id: "techno", bpmRange: "88-175", genreMinAgreement: 0.6, setMode: "arc", weights: { rhythm: 0.2, timbre: 0.2 } },
  { id: "dnb", bpmRange: "98-195", genreMinAgreement: 0.6, setMode: "arc", weights: { rhythm: 0.2 } },
  { id: "disco", bpmRange: "70-140", genreMinAgreement: 0.7, setMode: "smooth", weights: { harmony: 0.3, sound: 0.35 } },
  { id: "hiphop", bpmRange: "60-120", genreMinAgreement: 0.6, setMode: "smooth", weights: { tempo: 0.35, harmony: 0.25 } },
  { id: "open", bpmRange: "auto", genreMinAgreement: 0.5, setMode: "smooth", weights: { tempo: 0.35, harmony: 0.3, sound: 0.15 } },
  { id: "afterhours", bpmRange: "78-155", genreMinAgreement: 0.7, setMode: "smooth", weights: { energy: 0.3, sound: 0.4, timbre: 0.2 } },
  { id: "festival", bpmRange: "88-175", genreMinAgreement: 0.8, setMode: "arc", weights: { energy: 0.3 } },
];

const WEIGHTS_KEY = "dc.weights";
const SET_MODE_KEY = "dc.setMode";

function readLocal(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}
function writeLocal(key: string, value: string | null) {
  try {
    if (value == null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    /* per-device preference only */
  }
}

interface Snapshot {
  bpmRange: string;
  genreMinAgreement: string;
  djProfile: string;
  weights: string | null;
  setMode: string | null;
}

export function DjProfiles({ settings, onChanged }: { settings: Record<string, string>; onChanged: () => void }) {
  const { t, toast } = useApp();
  const act = useAction();
  const [previous, setPrevious] = useState<Snapshot | null>(null);
  const current = settings.djProfile || "";

  const describe = (p: DjProfile) => {
    const parts = [
      p.bpmRange === "auto" ? t("profiles.bpmAuto") : t("profiles.bpm", { range: p.bpmRange.replace("-", "–") }),
      t("profiles.agreement", { n: Math.round(p.genreMinAgreement * 100) }),
      t("profiles.setMode", { mode: t(`sets.mode.${p.setMode}` as MsgKey) }),
    ];
    const name = (k: string) => t(`disc.group.${k}` as MsgKey).toLowerCase();
    const changed = Object.entries(p.weights) as [keyof SimilarityWeights, number][];
    const more = changed.filter(([k, v]) => v > DEFAULT_WEIGHTS[k]).map(([k]) => name(k));
    const less = changed.filter(([k, v]) => v < DEFAULT_WEIGHTS[k]).map(([k]) => name(k));
    if (more.length) parts.push(t("profiles.weights", { list: more.join(", ") }));
    if (less.length) parts.push(t("profiles.weightsLess", { list: less.join(", ") }));
    if (!more.length && !less.length) parts.push(t("profiles.weightsDefault"));
    return parts;
  };

  const apply = async (p: DjProfile) => {
    const snap: Snapshot = {
      bpmRange: settings.bpmRange ?? "auto",
      genreMinAgreement: settings.genreMinAgreement ?? "0.6",
      djProfile: current,
      weights: readLocal(WEIGHTS_KEY),
      setMode: readLocal(SET_MODE_KEY),
    };
    const ok = await act(async () => {
      await api.setSetting("bpmRange", p.bpmRange);
      await api.setSetting("genreMinAgreement", String(p.genreMinAgreement));
      await api.setSetting("djProfile", p.id);
      return true;
    });
    if (!ok) return;
    writeLocal(WEIGHTS_KEY, JSON.stringify({ ...DEFAULT_WEIGHTS, ...p.weights }));
    writeLocal(SET_MODE_KEY, p.setMode);
    setPrevious(snap);
    onChanged();
    toast(t(snap.bpmRange !== p.bpmRange ? "profiles.appliedReanalyse" : "profiles.applied", { name: t(`profiles.${p.id}` as MsgKey) }), "ok");
  };

  const restore = async () => {
    if (!previous) return;
    await act(async () => {
      await api.setSetting("bpmRange", previous.bpmRange);
      await api.setSetting("genreMinAgreement", previous.genreMinAgreement);
      await api.setSetting("djProfile", previous.djProfile);
    });
    writeLocal(WEIGHTS_KEY, previous.weights);
    writeLocal(SET_MODE_KEY, previous.setMode);
    setPrevious(null);
    onChanged();
    toast(t("profiles.restored"), "ok");
  };

  return (
    <section className="panel">
      <div className="row space">
        <h2>{t("profiles.title")}</h2>
        {previous && (
          <button className="btn small ghost" onClick={restore}>
            ↶ {t("profiles.undo")}
          </button>
        )}
      </div>
      <p className="hint">{t("profiles.hint")}</p>
      <div className="profile-grid">
        {DJ_PROFILES.map((p) => (
          <button key={p.id} className={`profile-card ${current === p.id ? "active" : ""}`} onClick={() => apply(p)} aria-pressed={current === p.id}>
            <span className="profile-name">
              {t(`profiles.${p.id}` as MsgKey)}
              {current === p.id && <span className="badge">{t("profiles.active")}</span>}
            </span>
            <span className="profile-desc">{t(`profiles.${p.id}.desc` as MsgKey)}</span>
            <span className="profile-changes">
              {describe(p).map((d) => (
                <span key={d}>{d}</span>
              ))}
            </span>
          </button>
        ))}
      </div>
    </section>
  );
}

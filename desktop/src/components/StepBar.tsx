import { ArrowLeft } from "lucide-react";
import { useI18n } from "../lib/i18n";
export default function StepBar({
  label,
  onBack,
  busy = false,
}: {
  label: string;
  onBack: () => void;
  busy?: boolean;
}) {
  const { t } = useI18n();
  return (
    <div className="step-bar">
      <button
        className="icon-text-button"
        type="button"
        onClick={onBack}
        disabled={busy}
      >
        <ArrowLeft aria-hidden="true" />
        {t("返回")}
      </button>
      <span>{label}</span>
    </div>
  );
}

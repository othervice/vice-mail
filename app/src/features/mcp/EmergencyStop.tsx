import { useTranslation } from "react-i18next";
import { Button } from "../../components/Button";
import { ErrorNotice } from "../../components/ErrorNotice";
import { useMcpSettings, useSetMcpEnabled } from "./queries";

/** The emergency stop: visible wherever AI access matters while it is on. */
export function EmergencyStop() {
  const { t } = useTranslation();
  const settings = useMcpSettings();
  const setEnabled = useSetMcpEnabled();
  if (!settings.data?.enabled) return null;
  return (
    <div className="flex flex-col gap-2 rounded-card border border-danger p-4" data-testid="mcp-stop-block">
      <div>
        <Button
          variant="danger"
          disabled={setEnabled.isPending}
          onClick={() => {
            setEnabled.mutate(false);
          }}
          data-testid="mcp-stop"
        >
          {setEnabled.isPending ? t("mcp.stop.stopping") : t("mcp.stop.action")}
        </Button>
      </div>
      <p className="text-13 text-secondary">{t("mcp.stop.description")}</p>
      {setEnabled.isError && <ErrorNotice context={t("mcp.enable.failed")} error={setEnabled.error} />}
    </div>
  );
}

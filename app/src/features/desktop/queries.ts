import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import {
  desktopIpc,
  type BackgroundSettings,
  type CoreEvent,
  type DesktopTexts,
  type NotificationPause,
  type PauseNotifications,
} from "../../services/ipc";

/** Stage 21 (decisions 0069, 0070): running in the background, the pause, the login item. */
export const backgroundSettingsQueryKey = ["backgroundSettings"] as const;
export const autostartQueryKey = ["autostart"] as const;
export const notificationPauseQueryKey = ["notificationPause"] as const;

export const BACKGROUND_DEFAULTS: Required<BackgroundSettings> = { keepRunning: true, menuBarIcon: true };

export function useBackgroundSettings() {
  return useQuery({ queryKey: backgroundSettingsQueryKey, queryFn: desktopIpc.getBackgroundSettings });
}

export function useSetBackgroundSettings() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: desktopIpc.setBackgroundSettings,
    onSuccess: (stored) => {
      queryClient.setQueryData(backgroundSettingsQueryKey, stored);
    },
  });
}

/** The system's registration, asked again whenever the settings show (the user may change it there). */
export function useAutostart() {
  return useQuery({ queryKey: autostartQueryKey, queryFn: desktopIpc.getAutostart, staleTime: 0 });
}

export function useSetAutostart() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: desktopIpc.setAutostart,
    onSuccess: (state) => {
      queryClient.setQueryData(autostartQueryKey, state);
    },
    onError: () => {
      void queryClient.invalidateQueries({ queryKey: autostartQueryKey });
    },
  });
}

export function useNotificationPause() {
  return useQuery({ queryKey: notificationPauseQueryKey, queryFn: desktopIpc.getNotificationPause });
}

export function usePauseNotifications() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (choice: PauseNotifications) => desktopIpc.pauseNotifications(choice),
    onSuccess: (pause) => {
      queryClient.setQueryData(notificationPauseQueryKey, pause);
    },
  });
}

/** The tray menu may pause or resume while the settings show: `notificationPauseChanged` carries the pause. */
export function handleDesktopEvent(queryClient: QueryClient, event: CoreEvent): void {
  if (event.type === "notificationPauseChanged") {
    queryClient.setQueryData<NotificationPause>(notificationPauseQueryKey, event.pause);
  }
}

/** Whether the UI's clock reads 12-hour ("2:30 PM") in this language. */
function usesHour12(language: string): boolean {
  try {
    return new Intl.DateTimeFormat(language, { hour: "numeric" }).resolvedOptions().hour12 === true;
  } catch {
    return false;
  }
}

/** The tray menu's texts in the UI's language. */
export function useDesktopTextValues(): DesktopTexts {
  const { t, i18n } = useTranslation();
  return {
    open: t("desktop.tray.open"),
    newMessage: t("desktop.tray.newMessage"),
    pause: t("desktop.tray.pause"),
    pauseHour: t("desktop.tray.pauseHour"),
    pauseTomorrow: t("desktop.tray.pauseTomorrow"),
    resume: t("desktop.tray.resume"),
    pausedUntil: t("desktop.tray.pausedUntil"),
    pausedUntilTomorrow: t("desktop.tray.pausedUntilTomorrow"),
    quit: t("desktop.tray.quit"),
    tooltip: t("desktop.tray.tooltip"),
    tooltipUnread: t("desktop.tray.tooltipUnread"),
    hour12: usesHour12(i18n.language),
  };
}

/**
 * Sends the tray menu's texts at start and after every language change; the core keeps them, so
 * an app started in the background has them too. A failure only costs the translation.
 */
export function useDesktopTexts(): void {
  const texts = useDesktopTextValues();
  const key = JSON.stringify(texts);
  useEffect(() => {
    void desktopIpc.setDesktopTexts(JSON.parse(key) as DesktopTexts).catch(() => undefined);
  }, [key]);
}

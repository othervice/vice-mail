import type { Translation } from "./en";

// German texts. The type makes a missing or extra key a compile error; tests/i18n.test.ts checks
// the same at runtime.
export const de: Translation = {
  nav: {
    label: "Ansichten",
    inbox: "Posteingang",
    later: "Später",
    tasks: "Aufgaben",
    pinned: "Angepinnt",
    sent: "Gesendet",
    drafts: "Entwürfe",
    archive: "Archiv",
    trash: "Papierkorb",
    folders: "Ordner",
    savedViews: "Gespeicherte Ansichten",
    accounts: "Konten",
    settings: "Einstellungen",
  },
  mail: {
    search: "Suchen",
    compose: "Neue E-Mail",
    noAccount: {
      title: "Noch kein Konto",
      body: "Füge ein E-Mail-Konto hinzu, um hier deine Mails zu sehen.",
      action: "Konto hinzufügen",
    },
  },
  settings: {
    title: "Einstellungen",
    sections: "Bereiche der Einstellungen",
    general: {
      title: "Allgemein",
      language: {
        label: "Sprache",
        description: "Die Sprache von Menüs, Schaltflächen und Meldungen.",
        system: "System",
      },
    },
    accounts: {
      title: "Konten",
      empty: {
        title: "Noch keine Konten",
        body: "Das Einrichten von E-Mail-Konten kommt in einer späteren Version der App.",
      },
    },
    appearance: {
      title: "Darstellung",
      theme: {
        label: "Erscheinungsbild",
        description: "System folgt der hellen oder dunklen Einstellung deines Computers.",
        system: "System",
        light: "Hell",
        dark: "Dunkel",
      },
    },
    about: {
      title: "Über",
      version: "Version",
      commit: "Commit",
      buildDate: "Build-Datum",
      platform: "Plattform",
      license: "Lizenz",
      unknown: "Unbekannt",
    },
  },
  languages: {
    en: "English",
    de: "Deutsch",
  },
  platforms: {
    macos: "macOS",
    windows: "Windows",
    linux: "Linux",
  },
  errors: {
    details: "Technische Details",
    retry: "Erneut versuchen",
    context: {
      preferences: "Deine Einstellungen konnten nicht geladen werden.",
      savePreferences: "Deine Änderung konnte nicht gespeichert werden.",
      appInfo: "Die App-Informationen konnten nicht geladen werden.",
      events: "Live-Aktualisierungen aus dem App-Kern sind nicht verfügbar.",
    },
    codes: {
      internal: {
        title: "In der App ist etwas schiefgelaufen.",
        hint: "Versuche es erneut. Passiert es wieder, starte die App neu.",
      },
      invalid_argument: {
        title: "Die App hat eine ungültige Anfrage gesendet.",
        hint: "Das ist ein Fehler in der App. Versuche es erneut; passiert es wieder, melde ihn bitte.",
      },
      not_found: {
        title: "Das angeforderte Element gibt es nicht mehr.",
        hint: "Es wurde möglicherweise inzwischen entfernt.",
      },
      database: {
        title: "Die lokalen Daten konnten nicht gelesen oder geschrieben werden.",
        hint: "Prüfe, ob auf dem Datenträger Platz frei ist, und versuche es erneut.",
      },
      database_too_new: {
        title: "Die lokalen Daten stammen von einer neueren Version der App.",
        hint: "Aktualisiere die App auf die neueste Version, um sie zu öffnen.",
      },
    },
  },
};

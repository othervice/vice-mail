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
    account: "{{name}} ({{email}})",
    accountWithState: "{{name}} ({{email}}), {{state}}",
    accountState: {
      connecting: "verbindet",
      syncing: "gleicht ab",
      idle: "aktuell",
      offline: "offline",
      error: "kein Abgleich",
    },
  },
  sidebar: {
    folders: "Ordner von {{account}}",
    folderUnread_one: "{{name}}, {{count}} ungelesen",
    folderUnread_other: "{{name}}, {{count}} ungelesen",
    expand: "Unterordner von {{name}} zeigen",
    collapse: "Unterordner von {{name}} ausblenden",
    noFolders: "Die Ordner erscheinen nach dem ersten Abgleich.",
    loadFailed: "Die Ordner konnten nicht geladen werden.",
  },
  roles: {
    inbox: "Posteingang",
    sent: "Gesendet",
    drafts: "Entwürfe",
    archive: "Archiv",
    trash: "Papierkorb",
    junk: "Spam",
    flagged: "Markiert",
    all: "Alle Nachrichten",
    custom: "Gewöhnlicher Ordner",
  },
  mail: {
    search: "Suchen",
    compose: "Neue E-Mail",
    noAccount: {
      title: "Noch kein Konto",
      body: "Füge ein E-Mail-Konto hinzu, um hier deine Mails zu sehen.",
      action: "Konto hinzufügen",
    },
    notYet: {
      title: "Noch nicht verfügbar",
      body: "„{{view}}“ kommt in einer späteren Version der App.",
    },
    noRoleFolder: {
      title: "Kein Ordner für „{{role}}“",
      body: "Kein Ordner von {{account}} ist als „{{role}}“ markiert. Hat das Konto einen, wähle ihn in den Kontoeinstellungen.",
      action: "Kontoeinstellungen öffnen",
    },
    folderGone: {
      title: "Diesen Ordner gibt es nicht mehr",
      body: "Er wurde auf dem Server oder in einem anderen Programm entfernt.",
    },
  },
  list: {
    label: "Nachrichten in {{folder}}",
    loading: "Nachrichten werden geladen",
    loadingMore: "Weitere Nachrichten werden geladen",
    empty: {
      title: "Dieser Ordner ist leer",
      body: "Neue Nachrichten erscheinen hier, sobald sie ankommen.",
    },
    firstSync: {
      title: "Deine Mails werden geladen",
      body: "Der erste Abgleich von {{account}} läuft. Die Nachrichten erscheinen hier nach und nach, die neuesten zuerst.",
    },
    noSubject: "(Kein Betreff)",
    unknownSender: "(Unbekannter Absender)",
    to: "An: {{names}}",
    unread: "Ungelesen",
    flagged: "Markiert",
    attachment: "Mit Anhang",
    sections: {
      today: "Heute",
      yesterday: "Gestern",
      thisWeek: "Diese Woche",
    },
  },
  status: {
    connecting: "Verbindet …",
    syncing: "Gleicht ab …",
    progress: "{{done}} von {{total}}",
    offline: "Offline",
    allOffline: "Offline. Deine Mails auf diesem Computer bleiben verfügbar.",
    nextAttempt: "Nächster Versuch um {{time}}",
    retryNow: "Jetzt versuchen",
    error: "Kein Abgleich",
    errorContext: "{{account}} wird nicht abgeglichen.",
    changePassword: "Passwort ändern",
    accountSettings: "Kontoeinstellungen",
    retryFailed: "Die neue Verbindung konnte nicht gestartet werden.",
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
        body: "Füge ein E-Mail-Konto hinzu, um deine Mails mit dieser App zu lesen.",
      },
      add: "Konto hinzufügen",
      back: "Alle Konten",
      readOnly: "Nur lesend",
      rowStatus: "{{email}} · {{state}}",
      loadFailed: "Deine Konten konnten nicht geladen werden.",
      setup: {
        title: "Konto hinzufügen",
        intro:
          "Gib deine E-Mail-Adresse und dein Passwort ein. Für bekannte Anbieter kennt die App die Servereinstellungen selbst, ohne online zu gehen.",
        email: "E-Mail-Adresse",
        name: "Dein Name",
        nameDescription: "Sehen die Menschen, denen du schreibst. Du kannst ihn leer lassen.",
        password: "Passwort",
        passwordDescription: "Die App bewahrt es nur im Schlüsselbund dieses Computers auf.",
        invalidEmail: "Gib eine vollständige E-Mail-Adresse ein, zum Beispiel name@example.com.",
        required: "Erforderlich.",
        invalidPort: "Gib einen Port zwischen 1 und 65535 ein.",
        known: "Die Servereinstellungen für {{domain}} sind bekannt.",
        foundSrv: "Servereinstellungen in den DNS-Einträgen von {{domain}} gefunden.",
        foundAutoconfig: "Servereinstellungen unter autoconfig.{{domain}} gefunden.",
        checkFound: "Prüfe, ob diese Server zu deinem Anbieter gehören. Die Antworten der Online-Suche sind nicht abgesichert, ein manipuliertes Netz könnte woandershin verweisen.",
        unknown: "Die App kennt die Server von {{domain}} nicht.",
        onlineAction: "Einstellungen online suchen",
        onlineExplanation:
          "Die App fragt dann deinen DNS-Server nach den Mail-Einträgen von {{domain}} und lädt eine Einstellungsdatei von autoconfig.{{domain}}. Sonst kontaktiert sie nichts, und nur, wenn du klickst.",
        searching: "Sucht …",
        nothingFound: "Nichts gefunden. Gib die Einstellungen ein, die dein Anbieter nennt.",
        searchFailed: "Die Online-Suche ist fehlgeschlagen.",
        showServers: "Servereinstellungen bearbeiten",
        hideServers: "Servereinstellungen ausblenden",
        incoming: "Eingehende Mails (IMAP)",
        outgoing: "Ausgehende Mails (SMTP)",
        outgoingNote:
          "Senden kommt in einer späteren Version der App. Sie speichert diese Einstellungen schon dafür, prüft sie aber noch nicht.",
        host: "Server",
        port: "Port",
        username: "Benutzername",
        usernameValue: "Benutzername: {{username}}",
        security: "Sicherheit",
        securityDescription:
          "Die App verbindet sich nur verschlüsselt, dein Passwort geht also nie im Klartext über das Netz.",
        securityOptions: {
          tls: "TLS",
          starttls: "STARTTLS",
        },
        serverSummary: "{{host}}, Port {{port}}, {{security}}",
        readOnlyTitle: "Vorerst nur lesend",
        readOnlyBody:
          "Die App lädt und zeigt deine Mails, ändert aber nichts auf dem Server: keinen Gelesen-Status, kein Verschieben, kein Löschen. Änderungen für ein Konto zu erlauben, kommt in einer späteren Version.",
        providerHint: {
          gmail: {
            title: "Gmail braucht ein App-Passwort",
            body: "Dein normales Google-Passwort funktioniert hier nicht. Schalte in deinem Google-Konto die Bestätigung in zwei Schritten ein, erstelle unter Sicherheit ein App-Passwort und gib es oben als Passwort ein.",
          },
          icloud: {
            title: "iCloud Mail braucht ein app-spezifisches Passwort",
            body: "Das Passwort deines Apple Accounts funktioniert hier nicht. Erstelle auf account.apple.com unter Anmeldung und Sicherheit ein app-spezifisches Passwort und gib es oben als Passwort ein.",
          },
          yahoo: {
            title: "Yahoo Mail braucht ein App-Passwort",
            body: "Dein normales Yahoo-Passwort funktioniert hier nicht. Erstelle in deinem Yahoo-Konto unter Kontosicherheit ein App-Passwort und gib es oben als Passwort ein.",
          },
          fastmail: {
            title: "Fastmail braucht ein App-Passwort",
            body: "Erstelle in den Fastmail-Einstellungen unter Privacy & Security ein App-Passwort mit Zugriff auf IMAP und SMTP und gib es oben als Passwort ein.",
          },
          outlook: {
            title: "Outlook.com-Konten lassen sich noch nicht hinzufügen",
            body: "Microsoft lässt Apps nur über die eigene Anmeldeseite (OAuth) herein, nicht mit einem Passwort. Das lernt die App in einer späteren Version; bis dahin schlägt die Anmeldung hier fehl.",
          },
          proton: {
            title: "Proton Mail geht nur über Proton Mail Bridge",
            body: "Installiere und starte Proton Mail Bridge auf diesem Computer (braucht einen bezahlten Proton-Tarif). Gib den Benutzernamen und das Passwort ein, die Bridge anzeigt, nicht dein Proton-Passwort, und vertraue dem Zertifikat von Bridge in den Systemeinstellungen.",
          },
          gmx: {
            title: "Schalte IMAP zuerst bei GMX ein",
            body: "GMX lässt Mail-Programme erst herein, wenn du es erlaubst: Öffne in den Einstellungen des GMX-Webmailers POP3/IMAP und schalte den Zugriff ein. Gib dann oben dein GMX-Passwort ein.",
          },
          webde: {
            title: "Schalte IMAP zuerst bei WEB.DE ein",
            body: "WEB.DE lässt Mail-Programme erst herein, wenn du es erlaubst: Öffne in den Einstellungen des WEB.DE-Webmailers POP3/IMAP und schalte den Zugriff ein. Gib dann oben dein WEB.DE-Passwort ein.",
          },
          tonline: {
            title: "Telekom Mail braucht dein E-Mail-Passwort",
            body: "Nutze das eigene E-Mail-Passwort aus dem Telekom Kundencenter, nicht das Passwort deines Telekom-Logins.",
          },
        },
        test: "Verbindung testen",
        testing: "Testet …",
        testOk: "Die Verbindung funktioniert, und der Server hat die Anmeldung angenommen.",
        testIdle: "Neue Mails erscheinen sofort: Der Server unterstützt IDLE.",
        testNoIdle: "Der Server unterstützt kein IDLE, neue Mails erscheinen daher verzögert.",
        testFailed: "Der Verbindungstest ist fehlgeschlagen.",
        submit: "Konto hinzufügen",
        adding: "Fügt hinzu …",
        addFailed: "Das Konto konnte nicht hinzugefügt werden.",
        cancel: "Abbrechen",
      },
      detail: {
        status: "Status",
        states: {
          connecting: "Verbindet",
          syncing: "Gleicht ab",
          idle: "Aktuell",
          offline: "Offline, die App versucht es weiter",
          error: "Kein Abgleich, bis du etwas änderst",
        },
        lastSync: "Letzter vollständiger Abgleich: {{time}}",
        neverSynced: "Noch nicht vollständig abgeglichen.",
        retry: "Erneut versuchen",
        servers: "Server",
        password: {
          title: "Passwort",
          description:
            "Hast du dein Passwort beim Anbieter geändert, gib hier das neue ein. Die App prüft es, bevor sie es speichert.",
          label: "Neues Passwort",
          save: "Passwort speichern",
          saving: "Prüft …",
          saved: "Passwort gespeichert. Die App verbindet sich damit neu.",
          failed: "Das Passwort wurde nicht geändert.",
        },
        folders: {
          title: "Besondere Ordner",
          description:
            "Welcher Ordner welche Art von Mails enthält. Die App hat das selbst erkannt; ändere es, wenn etwas nicht stimmt.",
          none: "Keiner",
          source: {
            server: "Vom Server markiert",
            heuristic: "Am Ordnernamen erkannt",
            user: "Von dir gewählt",
          },
          failed: "Der Ordner konnte nicht geändert werden.",
          loadFailed: "Die Ordner konnten nicht geladen werden.",
          empty: "Die Ordner erscheinen nach dem ersten Abgleich.",
        },
        remove: {
          title: "Konto entfernen",
          description: "Entfernt das Konto und alles, was die App dafür gespeichert hat, von diesem Computer.",
          action: "Konto entfernen …",
          confirmTitle: "{{email}} aus dieser App entfernen?",
          confirmBody:
            "Die App löscht die Nachrichten, die sie für dieses Konto auf diesem Computer gespeichert hat, und sein Passwort aus dem Schlüsselbund. Deine Mails auf dem Server bleiben, wie sie sind; du kannst das Konto später wieder hinzufügen.",
          confirm: "Entfernen",
          cancel: "Konto behalten",
          removing: "Entfernt …",
          failed: "Das Konto konnte nicht entfernt werden.",
        },
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
      messages: "Die Nachrichten dieses Ordners konnten nicht geladen werden.",
      moreMessages: "Weitere Nachrichten konnten nicht geladen werden.",
    },
    codes: {
      internal: {
        title: "In der App ist etwas schiefgelaufen.",
        hint: "Versuche es erneut. Passiert es wieder, starte die App neu.",
      },
      invalid_argument: {
        title: "Ein Wert wurde nicht angenommen.",
        hint: "Prüfe deine Eingaben. Sieht alles richtig aus, ist das ein Fehler in der App; melde ihn bitte.",
      },
      not_found: {
        title: "Das angeforderte Element gibt es nicht mehr.",
        hint: "Es wurde möglicherweise inzwischen entfernt, zum Beispiel ein Ordner auf dem Server.",
      },
      database: {
        title: "Die lokalen Daten konnten nicht gelesen oder geschrieben werden.",
        hint: "Prüfe, ob auf dem Datenträger Platz frei ist, und versuche es erneut.",
      },
      database_too_new: {
        title: "Die lokalen Daten stammen von einer neueren Version der App.",
        hint: "Aktualisiere die App auf die neueste Version, um sie zu öffnen.",
      },
      auth_failed: {
        title: "Der Server hat Benutzername oder Passwort nicht angenommen.",
        hint: "Prüfe Passwort und Benutzernamen. Gmail und iCloud brauchen ein App-Passwort statt deines normalen Passworts.",
      },
      cert_untrusted: {
        title: "Dem Zertifikat des Servers wird nicht vertraut.",
        hint: "Dein Computer vertraut dem Zertifikat nicht, zum Beispiel weil es selbst signiert ist. Ist es dein eigener Server, füge sein Zertifikat den vertrauenswürdigen Zertifikaten des Systems hinzu und versuche es erneut. Sonst prüfe den Servernamen.",
      },
      cert_name_mismatch: {
        title: "Das Zertifikat des Servers gilt für einen anderen Namen.",
        hint: "Gib den Servernamen genau so ein, wie dein Anbieter ihn nennt, zum Beispiel imap.example.com statt example.com.",
      },
      cert_expired: {
        title: "Das Zertifikat des Servers ist abgelaufen oder noch nicht gültig.",
        hint: "Prüfe Datum und Uhrzeit deines Computers. Stimmen sie, muss der Betreiber des Servers das Zertifikat erneuern.",
      },
      tls_failed: {
        title: "Eine sichere Verbindung zum Server kam nicht zustande.",
        hint: "Prüfe Port und Sicherheit: TLS nutzt meist Port 993, STARTTLS Port 143.",
      },
      starttls_unavailable: {
        title: "Der Server bietet auf diesem Port keine Verschlüsselung an.",
        hint: "Wähle TLS mit Port 993. Die App sendet dein Passwort nie unverschlüsselt.",
      },
      connection_refused: {
        title: "Der Server hat die Verbindung abgelehnt.",
        hint: "Prüfe Servername und Port. Der Server kann auch gerade nicht erreichbar sein.",
      },
      connection_timeout: {
        title: "Der Server hat nicht rechtzeitig geantwortet.",
        hint: "Prüfe deine Internetverbindung und den Port. Eine Firewall kann die Verbindung blockieren.",
      },
      connection_lost: {
        title: "Die Verbindung zum Server wurde unterbrochen.",
        hint: "Prüfe deine Internetverbindung. Die App verbindet sich von selbst neu.",
      },
      dns_failed: {
        title: "Der Servername wurde nicht gefunden.",
        hint: "Prüfe die Schreibweise des Servernamens und deine Internetverbindung.",
      },
      protocol: {
        title: "Der Server hat eine Antwort geschickt, die die App nicht versteht.",
        hint: "Versuche es erneut. Passiert es wieder, melde es bitte mit den technischen Details.",
      },
      server_rejected: {
        title: "Der Server hat die Anfrage abgelehnt.",
        hint: "Versuche es später erneut. Die technischen Details nennen vielleicht den Grund.",
      },
      read_only: {
        title: "Änderungen sind für dieses Konto ausgeschaltet.",
        hint: "Neue Konten lesen vorerst nur. Änderungen zu erlauben, kommt in einer späteren Version der App.",
      },
      unsupported_server: {
        title: "Dem Server fehlt eine Funktion, die die App braucht.",
        hint: "Die App braucht einen IMAP-Server (IMAP4rev1). Prüfe, ob du den Server für eingehende Mails eingegeben hast.",
      },
      keychain: {
        title: "Das Passwort konnte nicht im Schlüsselbund des Systems gespeichert oder gelesen werden.",
        hint: "Sorge dafür, dass der Schlüsselbund entsperrt ist und die App ihn nutzen darf, und versuche es erneut.",
      },
      account_exists: {
        title: "Dieses Konto ist schon eingerichtet.",
        hint: "Du findest es unter Einstellungen, Konten.",
      },
    },
  },
};

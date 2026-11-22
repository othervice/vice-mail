// Base texts (English). Every user-visible string of the UI lives here and in de.ts.
// The keys of `nav` are the mail view ids (see features/mail/views.ts).
export const en = {
  nav: {
    label: "Views",
    inbox: "Inbox",
    later: "Later",
    tasks: "Tasks",
    pinned: "Pinned",
    sent: "Sent",
    drafts: "Drafts",
    archive: "Archive",
    trash: "Trash",
    folders: "Folders",
    savedViews: "Saved views",
    accounts: "Accounts",
    settings: "Settings",
  },
  mail: {
    search: "Search",
    compose: "New message",
    noAccount: {
      title: "No account yet",
      body: "Add an email account to see your mail here.",
      action: "Add account",
    },
  },
  settings: {
    title: "Settings",
    sections: "Settings sections",
    general: {
      title: "General",
      language: {
        label: "Language",
        description: "The language of menus, buttons and messages.",
        system: "System",
      },
    },
    accounts: {
      title: "Accounts",
      empty: {
        title: "No accounts yet",
        body: "Setting up email accounts arrives in a later version of the app.",
      },
    },
    appearance: {
      title: "Appearance",
      theme: {
        label: "Theme",
        description: "System follows the light or dark setting of your computer.",
        system: "System",
        light: "Light",
        dark: "Dark",
      },
    },
    about: {
      title: "About",
      version: "Version",
      commit: "Commit",
      buildDate: "Build date",
      platform: "Platform",
      license: "License",
      unknown: "Unknown",
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
    details: "Technical details",
    retry: "Try again",
    context: {
      preferences: "Your settings could not be loaded.",
      savePreferences: "Your change could not be saved.",
      appInfo: "App information could not be loaded.",
      events: "Live updates from the app core are not available.",
    },
    codes: {
      internal: {
        title: "Something went wrong inside the app.",
        hint: "Try again. If it keeps happening, restart the app.",
      },
      invalid_argument: {
        title: "The app sent an invalid request.",
        hint: "This is a bug in the app. Try again; if it keeps happening, please report it.",
      },
      not_found: {
        title: "The requested item no longer exists.",
        hint: "It may have been removed in the meantime.",
      },
      database: {
        title: "The local data could not be read or written.",
        hint: "Check that your disk has free space, then try again.",
      },
      database_too_new: {
        title: "The local data was created by a newer version of the app.",
        hint: "Update the app to the latest version to open it.",
      },
    },
  },
} as const;

type DeepStrings<T> = { readonly [K in keyof T]: T[K] extends string ? string : DeepStrings<T[K]> };

/** Shape every language has to provide: the same keys as English, any text. */
export type Translation = DeepStrings<typeof en>;

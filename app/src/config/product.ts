// The product name is a working title. Besides app/src-tauri/tauri.conf.json this is the only
// place where it is written; everything else reads it from here.
export const PRODUCT_NAME = "Vice Mail";

/** Where the source code is published (decision 0035), shown on the About page. */
export const PROJECT_URL = "https://github.com/othervice/vice-mail";

/**
 * Who publishes the app (decision 0035, addendum of 24.11.2026). The installer and the macOS
 * bundle take it from `bundle.publisher` and `bundle.copyright` in app/src-tauri/tauri.conf.json.
 */
export const PUBLISHER = "BITWATER LLC";

/** SPDX identifier of the license the app is distributed under. */
export const LICENSE_SPDX = "GPL-3.0-or-later";

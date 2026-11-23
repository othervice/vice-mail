#!/usr/bin/env bash
# Prepares this Mac to sign and notarise the macOS build (docs/RELEASING.md, "macOS signing and
# notarisation"). Interactive; every step can run again and skips what is done already.
#
#   scripts/apple-signing-setup.sh              guided: shows the state and does the next open step
#   scripts/apple-signing-setup.sh csr          1. private key in the login keychain and a
#                                                  certificate signing request (CSR) for Apple
#   scripts/apple-signing-setup.sh csr --new    the same, replacing a request that is still pending
#   scripts/apple-signing-setup.sh cert <file.cer>
#                                               2. import the "Developer ID Application" certificate
#   scripts/apple-signing-setup.sh notary       3. store the notarisation credentials as the
#                                                  notarytool keychain profile `vicemail-notary`
#   scripts/apple-signing-setup.sh --check      check identity and profile (prints no secrets);
#                                               exit 0 only if a signed, notarised build can run
#
# VICEMAIL_CSR_DIR is where the CSR goes (default ~/Downloads). VICEMAIL_NOTARY_PROFILE names the
# notarytool profile (default vicemail-notary). VICEMAIL_SIGNING_KEYCHAIN uses another keychain
# file instead of the login keychain for the key and the certificate (tests); the notarytool
# profile always lives in the login keychain.
#
# Nothing secret reaches the repository, a file, an environment file, a log or a command line:
# the private key is made in memory and goes into the keychain through a pipe; the app-specific
# password is asked for by notarytool itself; the API key file is read by notarytool, which keeps
# its own copy in the keychain.
set -euo pipefail

usage() {
  awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"
}

say() { printf '%s\n' "$*"; }
warn() { printf 'apple-signing-setup: %s\n' "$*" >&2; }
fail() {
  warn "$*"
  exit 1
}

[[ "$(uname -s)" == Darwin ]] || fail "this sets up the macOS keychain; run it on the build Mac"
for tool in security codesign curl shasum; do
  command -v "$tool" >/dev/null || fail "$tool missing (part of macOS)"
done
# LibreSSL from macOS: its RSA key format is the one `security import -f openssl` reads.
openssl=/usr/bin/openssl
[[ -x "$openssl" ]] || fail "/usr/bin/openssl missing (part of macOS)"

PROFILE="${VICEMAIL_NOTARY_PROFILE:-vicemail-notary}"
[[ "$PROFILE" =~ ^[A-Za-z0-9._-]+$ ]] || fail "bad notarytool profile name '$PROFILE'"
KEYCHAIN="${VICEMAIL_SIGNING_KEYCHAIN:-$HOME/Library/Keychains/login.keychain-db}"
[[ -f "$KEYCHAIN" ]] || fail "no keychain at $KEYCHAIN"
CSR_DIR="${VICEMAIL_CSR_DIR:-$HOME/Downloads}"
CSR="$CSR_DIR/ViceMail-DeveloperID.certSigningRequest"
PORTAL_CERTS="https://developer.apple.com/account/resources/certificates/add"
PORTAL_KEYS="https://appstoreconnect.apple.com/access/integrations/api"

tmp="$(mktemp -d)"
hidden_p8=""
cleanup() {
  # An API key file hidden for the check in step 3 always comes back, even after Ctrl-C.
  [[ -z "$hidden_p8" || ! -f "$hidden_p8" ]] || mv "$hidden_p8" "${hidden_p8%.vicemail-check}"
  rm -rf "$tmp"
}
trap cleanup EXIT

ask() { # prompt, default: prints the answer
  local answer
  read -r -p "$1${2:+ [$2]}: " answer || answer=""
  printf '%s' "${answer:-${2:-}}"
}
confirm() { # prompt, default y or n
  local answer
  read -r -p "$1 [$([[ $2 == y ]] && printf 'Y/n' || printf 'y/N')] " answer || answer=""
  answer="${answer:-$2}"
  [[ "$answer" == [yY] || "$answer" == [yY][eE][sS] ]]
}
# A path typed or dragged into Terminal: quotes, backslash escapes and ~ removed.
clean_path() {
  local path="$1"
  path="${path#\'}"
  path="${path%\'}"
  path="${path#\"}"
  path="${path%\"}"
  path="${path//\\ / }"
  path="${path/#\~/$HOME}"
  printf '%s' "$path"
}

# Over SSH the login keychain is often locked, and nothing can show the unlock dialog.
ensure_unlocked() {
  security show-keychain-info "$KEYCHAIN" >/dev/null 2>&1 && return 0
  say "The keychain $KEYCHAIN is locked. Unlock it with the Mac's login password:"
  security unlock-keychain "$KEYCHAIN" || fail "the keychain stays locked"
}

# SHA-1 of an RSA public key (PKCS#1), which the keychain stores as the key's "application
# label": it links a private key, the CSR made from it and the certificate Apple issues for it.
pubkey_hash() { # reads a PEM public key on stdin
  "$openssl" rsa -pubin -RSAPublicKey_out -outform DER 2>/dev/null | shasum -a 1 | cut -d' ' -f1 |
    tr 'a-f' 'A-F'
}
csr_key_hash() { "$openssl" req -in "$1" -noout -pubkey 2>/dev/null | pubkey_hash; }
key_in_keychain() { # hash
  security find-key -t private "$KEYCHAIN" 2>/dev/null | grep -q "0x00000006 <blob>=0x$1 "
}

# Valid "Developer ID Application" identities as "<SHA-1> <name>".
dev_id_identities() {
  { security find-identity -v -p codesigning "$KEYCHAIN" 2>/dev/null || true; } |
    sed -n 's/^ *[0-9][0-9]*) \([0-9A-F]\{40\}\) "\(Developer ID Application: .*\)"$/\1 \2/p' | sort -u
}
count_lines() { printf '%s' "$1" | grep -c . || true; }

pending_csr() { # true if the CSR file exists and its private key is in the keychain
  [[ -f "$CSR" ]] || return 1
  local hash
  hash="$(csr_key_hash "$CSR")"
  [[ -n "$hash" ]] && key_in_keychain "$hash"
}

profile_works() { # asks Apple; prints nothing of the answer
  xcrun notarytool history --keychain-profile "$PROFILE" --output-format json >/dev/null 2>"$tmp/notary.log"
}

# Signs a copy of /usr/bin/true like a release build does: proves that codesign may use the key
# without a dialog and that Apple's timestamp server answers.
probe_sign() { # SHA-1
  cp /usr/bin/true "$tmp/probe"
  codesign --force --sign "$1" --keychain "$KEYCHAIN" --options runtime --timestamp "$tmp/probe" \
    >/dev/null 2>"$tmp/codesign.log"
}

upload_instructions() {
  cat <<EOF

Next, in a browser (signed in with the Apple Account of the developer membership):
  1. $PORTAL_CERTS
     (Certificates, Identifiers & Profiles -> Certificates -> "+")
  2. Choose "Developer ID Application", then Continue.
  3. Profile type: "G2 Sub-CA (Xcode 11.4.1 or later)". Choose the file
       $CSR
     (copy it to the computer with the browser first, e.g. scp or VS Code's "Download..."),
     then Continue and Download. Apple names the file developerID_application.cer.
  4. Bring the .cer back to this Mac and run:
       scripts/apple-signing-setup.sh cert ~/Downloads/developerID_application.cer
Only the Account Holder of the membership can create Developer ID certificates.
EOF
}

# --- 1. Private key and certificate signing request ----------------------------------------

step_csr() { # [--new]
  local identities
  identities="$(dev_id_identities)"
  if [[ -n "$identities" && "${1:-}" != --new ]]; then
    say "A Developer ID Application identity is already in the keychain; no new request needed:"
    say "$identities"
    return 0
  fi
  if pending_csr && [[ "${1:-}" != --new ]]; then
    say "A request is already waiting for Apple: $CSR (its private key is in the keychain)."
    say "Upload that file; 'csr --new' makes a fresh key and request instead."
    upload_instructions
    return 0
  fi
  if [[ -f "$CSR" ]]; then
    confirm "Replace $CSR with a new request (the old one is then of no use)?" n || fail "stopped"
  fi
  ensure_unlocked

  say "The request names you; Apple only checks the file, it puts the account's name into the"
  say "certificate itself. Nothing of this is stored outside the request file."
  local email name country
  email="$(ask "Email address of your Apple Account" "$(git config --global user.email 2>/dev/null || true)")"
  name="$(ask "Common name (your name)" "$(git config --global user.name 2>/dev/null || true)")"
  country="$(ask "Country code" DE)"
  [[ "$email" =~ ^[^[:space:]/\\]+@[^[:space:]/\\]+$ ]] || fail "'$email' does not look like an email address"
  [[ -n "$name" && "$name" != *[/\\\"+=]* ]] || fail "the name must not be empty or contain / \\ \" + or ="
  [[ "$country" =~ ^[A-Z]{2}$ ]] || fail "the country code has two capital letters (e.g. DE)"

  # The private key exists only in this shell's memory and in the keychain. It reaches openssl
  # and security through pipes (the builtin printf puts no argument on a command line); the pause
  # lets the whole key arrive in the pipe before `security` looks at its size.
  local key hash
  key="$("$openssl" genrsa 2048 2>/dev/null)" || fail "openssl could not make an RSA key"
  [[ "$key" == "-----BEGIN RSA PRIVATE KEY-----"* ]] || fail "unexpected key format from $openssl"
  mkdir -p "$CSR_DIR"
  printf '%s\n' "$key" | "$openssl" req -new -utf8 -sha256 -key /dev/stdin \
    -subj "/emailAddress=$email/CN=$name/C=$country" -out "$tmp/request.csr" ||
    fail "openssl could not write the request"
  printf '%s\n' "$key" | {
    sleep 1
    security import /dev/stdin -k "$KEYCHAIN" -t priv -f openssl -T /usr/bin/codesign >/dev/null
  } || fail "importing the private key into $KEYCHAIN failed"
  key=""
  hash="$(csr_key_hash "$tmp/request.csr")"
  key_in_keychain "$hash" || fail "the private key did not arrive in the keychain"
  mv -f "$tmp/request.csr" "$CSR"
  say ""
  say "Private key stored in $KEYCHAIN (Keychain Access lists it under Keys as"
  say "\"Imported Private Key\"; codesign may use it). Certificate signing request:"
  say "  $CSR"
  say "The request is not secret. Its text, if copying the file is awkward:"
  cat "$CSR"
  upload_instructions
}

# --- 2. The certificate --------------------------------------------------------------------

step_cert() { # file
  local file pem subject issuer hash name sha
  [[ -n "${1:-}" ]] || fail "usage: scripts/apple-signing-setup.sh cert <developerID_application.cer>"
  file="$(clean_path "$1")"
  [[ -f "$file" ]] || fail "no file at $file"
  pem="$tmp/cert.pem"
  "$openssl" x509 -inform DER -in "$file" -out "$pem" 2>/dev/null ||
    "$openssl" x509 -in "$file" -out "$pem" 2>/dev/null || fail "$file is not a certificate"
  subject="$("$openssl" x509 -in "$pem" -noout -subject -nameopt RFC2253 2>/dev/null)"
  issuer="$("$openssl" x509 -in "$pem" -noout -issuer -nameopt RFC2253 2>/dev/null)"
  [[ "$subject" == *"CN=Developer ID Application: "* ]] ||
    fail "this is not a Developer ID Application certificate ($subject)"
  hash="$("$openssl" x509 -in "$pem" -noout -pubkey | pubkey_hash)"
  ensure_unlocked
  key_in_keychain "$hash" ||
    fail "the private key of this certificate is not in $KEYCHAIN. Was the request made on another Mac or with another keychain?"

  security import "$pem" -k "$KEYCHAIN" -t cert >"$tmp/import.log" 2>&1 ||
    grep -q 'already exists' "$tmp/import.log" || {
    cat "$tmp/import.log" >&2
    fail "importing the certificate failed"
  }

  sha="$("$openssl" x509 -in "$pem" -noout -fingerprint -sha1 | sed 's/^.*=//; s/://g' | tr 'a-f' 'A-F')"
  if ! dev_id_identities | grep -q "^$sha "; then
    # macOS trusts the certificate only with Apple's intermediate certificate next to it.
    local ca_url=https://www.apple.com/certificateauthority/DeveloperIDCA.cer
    [[ "$issuer" != *"OU=G2"* ]] || ca_url=https://www.apple.com/certificateauthority/DeveloperIDG2CA.cer
    say "Fetching Apple's intermediate certificate $ca_url"
    curl -fsSL --proto '=https' -o "$tmp/ca.cer" "$ca_url" || fail "download failed: $ca_url"
    "$openssl" x509 -inform DER -in "$tmp/ca.cer" -noout -subject 2>/dev/null |
      grep -q 'Developer ID Certification Authority' || fail "unexpected content at $ca_url"
    security import "$tmp/ca.cer" -k "$KEYCHAIN" -t cert >"$tmp/import.log" 2>&1 ||
      grep -q 'already exists' "$tmp/import.log" || {
      cat "$tmp/import.log" >&2
      fail "importing Apple's intermediate certificate failed"
    }
    dev_id_identities | grep -q "^$sha " ||
      fail "the identity is still not valid: security find-identity -v -p codesigning shows why"
  fi
  name="$(dev_id_identities | grep "^$sha " | cut -c42-)"
  probe_sign "$sha" || {
    cat "$tmp/codesign.log" >&2
    fail "codesign cannot use the new identity yet (keychain locked, or a dialog nobody can answer over SSH)"
  }
  say "Signing identity ready: $name"
  say "  SHA-1 $sha, valid until $("$openssl" x509 -in "$pem" -noout -enddate | sed 's/^notAfter=//')"
  say "scripts/build-macos.sh finds it by itself."
}

# --- 3. Notarisation credentials -----------------------------------------------------------

step_notary() {
  if profile_works; then
    say "The notarytool profile '$PROFILE' exists and Apple accepts it."
    confirm "Replace it?" n || return 0
  fi
  say ""
  say "Credentials for Apple's notary service, stored as the notarytool keychain profile '$PROFILE':"
  say "  1) App Store Connect API key (recommended: no Apple Account password involved, revocable)"
  say "  2) Apple Account + app-specific password"
  case "$(ask "Choice" 1)" in
    1) notary_api_key ;;
    2) notary_apple_id ;;
    *) fail "choose 1 or 2" ;;
  esac
}

notary_api_key() {
  cat <<EOF

The key comes from App Store Connect: $PORTAL_KEYS
  Users and Access -> Integrations -> App Store Connect API -> Team Keys -> "+" (Generate API Key),
  name "Vice Mail notarisation", access "Developer". (On first use the Account Holder requests API
  access there once.) Download the AuthKey_<KEY ID>.p8 file; Apple offers it only once.
  The Issuer ID is shown above the table of keys.
EOF
  local p8 key_id issuer default_id=""
  p8="$(clean_path "$(ask "Path to AuthKey_<KEY ID>.p8 on this Mac" "")")"
  [[ -f "$p8" ]] || fail "no file at '$p8'"
  grep -q -- '-----BEGIN PRIVATE KEY-----' "$p8" || fail "$p8 is not an App Store Connect API key (.p8)"
  [[ "$(basename "$p8")" =~ ^AuthKey_([A-Z0-9]+)\.p8$ ]] && default_id="${BASH_REMATCH[1]}"
  key_id="$(ask "Key ID" "$default_id")"
  [[ "$key_id" =~ ^[A-Z0-9]{8,20}$ ]] || fail "the Key ID has capital letters and digits (e.g. 2X9R4HXF34)"
  issuer="$(ask "Issuer ID (a UUID)" "")"
  issuer="$(printf '%s' "$issuer" | tr 'A-F' 'a-f')"
  [[ "$issuer" =~ ^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$ ]] ||
    fail "the Issuer ID is a UUID like 57246542-96fe-1a63-e053-0824d011072a"
  # notarytool reads the key file and checks the credentials with Apple before it stores them.
  xcrun notarytool store-credentials "$PROFILE" --key "$p8" --key-id "$key_id" --issuer "$issuer" ||
    fail "notarytool could not store the profile (wrong Key ID or Issuer ID, revoked key, or a key without the Developer role?)"
  # The profile holds its own copy of the key. Prove that without the file before offering to
  # delete it: hide the file for a moment and ask Apple once more.
  hidden_p8="$p8.vicemail-check"
  mv "$p8" "$hidden_p8"
  if profile_works; then
    mv "$hidden_p8" "$p8"
    hidden_p8=""
  else
    mv "$hidden_p8" "$p8"
    hidden_p8=""
    cat "$tmp/notary.log" >&2
    fail "the stored profile does not work without $p8; keep the file and see docs/RELEASING.md"
  fi
  say ""
  say "The profile '$PROFILE' works without the file. Keep a backup of the .p8 in your password"
  say "manager (Apple will not offer it again), then this copy on the Mac is no longer needed."
  if confirm "Delete $p8 from this Mac now?" n; then
    rm -f "$p8"
    say "Deleted. (Check the Trash and other copies, e.g. in the browser's download folder.)"
  else
    say "Kept $p8. Delete it yourself once it is backed up."
  fi
}

notary_apple_id() {
  cat <<EOF

Make an app-specific password at https://account.apple.com -> Sign-In and Security ->
App-Specific Passwords. The Team ID is on https://developer.apple.com/account -> Membership details.
notarytool asks for the password itself; it never appears on a command line or in a file.
EOF
  local apple_id team
  apple_id="$(ask "Apple Account (email)" "")"
  [[ "$apple_id" =~ ^[^[:space:]]+@[^[:space:]]+$ ]] || fail "'$apple_id' does not look like an email address"
  team="$(ask "Team ID" "$(dev_id_identities | sed -n 's/.*(\([A-Z0-9]\{10\}\))$/\1/p' | head -1)")"
  [[ "$team" =~ ^[A-Z0-9]{10}$ ]] || fail "the Team ID has ten capital letters and digits"
  xcrun notarytool store-credentials "$PROFILE" --apple-id "$apple_id" --team-id "$team" ||
    fail "notarytool could not store the profile"
}

# --- Check ---------------------------------------------------------------------------------

cert_pem() { # SHA-1, name: the certificate as PEM
  security find-certificate -a -Z -p -c "$2" "$KEYCHAIN" 2>/dev/null |
    awk -v sha="$1" '/^SHA-1 hash: / { keep = ($3 == sha) } /^SHA-/ { next } keep'
}

check() {
  local ok=0 identities sha name end
  say "Keychain: $KEYCHAIN"
  identities="$(dev_id_identities)"
  if [[ -z "$identities" ]]; then
    say "  identity:  MISSING (no valid Developer ID Application identity)"
    if pending_csr; then
      say "             a request is waiting for Apple: $CSR"
    fi
    ok=1
  fi
  while read -r sha name <&3; do
    [[ -n "$sha" ]] || continue
    cert_pem "$sha" "$name" >"$tmp/identity.pem"
    end="$("$openssl" x509 -in "$tmp/identity.pem" -noout -enddate 2>/dev/null | sed 's/^notAfter=//')"
    say "  identity:  $name"
    say "             SHA-1 $sha, valid until ${end:-unknown}"
    if [[ -n "$end" ]] &&
      ! "$openssl" x509 -in "$tmp/identity.pem" -noout -checkend $((60 * 86400)) >/dev/null 2>&1; then
      say "             expires within 60 days: renew it (docs/RELEASING.md)"
    fi
    if probe_sign "$sha"; then
      say "             codesign can use it (test signature with secure timestamp ok)"
    else
      say "             codesign CANNOT use it: $(tr '\n' ' ' <"$tmp/codesign.log")"
      ok=1
    fi
  done 3<<<"$identities"
  if [[ "$(count_lines "$identities")" -gt 1 ]]; then
    say "             several identities: build-macos.sh needs VICEMAIL_SIGNING_IDENTITY=<SHA-1>"
  fi
  if profile_works; then
    say "  notary:    profile '$PROFILE' accepted by Apple"
  else
    say "  notary:    profile '$PROFILE' MISSING or rejected: $(tr '\n' ' ' <"$tmp/notary.log" | cut -c1-200)"
    ok=1
  fi
  if [[ $ok -eq 0 ]]; then
    say "Ready: scripts/build-macos.sh signs, notarises and staples."
  else
    say "Not ready: run scripts/apple-signing-setup.sh for the next step."
  fi
  return $ok
}

# --- Guided --------------------------------------------------------------------------------

guided() {
  say "Vice Mail: Developer ID signing and notarisation for this Mac (docs/RELEASING.md)."
  if [[ -z "$(dev_id_identities)" ]]; then
    if pending_csr; then
      say "Step 2 of 3: import the certificate Apple issued for $CSR."
      local cer
      cer="$(ask "Path to developerID_application.cer (empty: not downloaded yet)" "")"
      if [[ -z "$cer" ]]; then
        upload_instructions
        return 0
      fi
      step_cert "$cer"
    else
      say "Step 1 of 3: a private key for the certificate and a request for Apple."
      confirm "Create them now?" y || return 0
      step_csr
      return 0
    fi
  else
    say "Steps 1 and 2 done: $(dev_id_identities | cut -c42- | head -1)"
  fi
  if ! profile_works; then
    say "Step 3 of 3: notarisation credentials."
    step_notary
  fi
  say ""
  check
}

case "${1:-}" in
  "") guided ;;
  csr)
    [[ $# -le 2 && "${2:---new}" == --new ]] || fail "usage: scripts/apple-signing-setup.sh csr [--new]"
    step_csr "${2:-}"
    ;;
  cert)
    [[ $# -eq 2 ]] || fail "usage: scripts/apple-signing-setup.sh cert <developerID_application.cer>"
    step_cert "$2"
    ;;
  notary) step_notary ;;
  --check) check ;;
  -h | --help) usage ;;
  *)
    usage >&2
    exit 2
    ;;
esac

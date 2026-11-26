; Stage 9 (decision 0020): registers the app as a handler of mailto: links for the current user,
; so it appears under Settings > Apps > Default apps. Windows lets only the user choose the
; default there; the app opens that page ("Als Standard festlegen"). Names come from the
; Tauri configuration (BUNDLEID, PRODUCTNAME, MAINBINARYNAME), not from this file.

!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr HKCU "Software\Classes\${BUNDLEID}.mailto" "" "URL:MailTo Protocol"
  WriteRegStr HKCU "Software\Classes\${BUNDLEID}.mailto" "URL Protocol" ""
  WriteRegStr HKCU "Software\Classes\${BUNDLEID}.mailto\DefaultIcon" "" "$INSTDIR\${MAINBINARYNAME}.exe,0"
  WriteRegStr HKCU "Software\Classes\${BUNDLEID}.mailto\shell\open\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" "%1"'
  WriteRegStr HKCU "Software\${BUNDLEID}\Capabilities" "ApplicationName" "${PRODUCTNAME}"
  WriteRegStr HKCU "Software\${BUNDLEID}\Capabilities" "ApplicationDescription" "${PRODUCTNAME}"
  WriteRegStr HKCU "Software\${BUNDLEID}\Capabilities\URLAssociations" "mailto" "${BUNDLEID}.mailto"
  WriteRegStr HKCU "Software\RegisteredApplications" "${PRODUCTNAME}" "Software\${BUNDLEID}\Capabilities"
  ; Stage 22 (decision 0080): tell the shell that the icon changed, after an install and after an
  ; update alike. An update replaces ${MAINBINARYNAME}.exe in place and Tauri's template keeps the
  ; existing shortcuts (no re-creation in update mode), so a pinned taskbar button or a Start
  ; entry went on showing the icon Windows had cached for the old file (the orange one after
  ; 0.1.2). First SHCNE_UPDATEITEM (0x2000) for the program and every shortcut to it that
  ; exists, by path (SHCNF_PATHW 0x5); then SHCNE_ASSOCCHANGED (0x08000000, SHCNF_IDLIST), on
  ; which Explorer drops its cached icons; each with SHCNF_FLUSH (0x1000), so the shell has
  ; handled it before the installer closes.
  System::Call 'shell32::SHChangeNotify(i 0x2000, i 0x1005, w "$INSTDIR\${MAINBINARYNAME}.exe", p 0)'
  !insertmacro VICE_NOTIFY_SHORTCUT "$SMPROGRAMS\${PRODUCTNAME}.lnk"
  !insertmacro VICE_NOTIFY_SHORTCUT "$DESKTOP\${PRODUCTNAME}.lnk"
  !insertmacro VICE_NOTIFY_SHORTCUT "$APPDATA\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar\${PRODUCTNAME}.lnk"
  !insertmacro VICE_NOTIFY_SHORTCUT "$APPDATA\Microsoft\Internet Explorer\Quick Launch\User Pinned\StartMenu\${PRODUCTNAME}.lnk"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0x1000, p 0, p 0)'
!macroend

; SHCNE_UPDATEITEM for one shortcut, if it exists (see NSIS_HOOK_POSTINSTALL).
!macro VICE_NOTIFY_SHORTCUT path
  ${If} ${FileExists} "${path}"
    System::Call 'shell32::SHChangeNotify(i 0x2000, i 0x1005, w "${path}", p 0)'
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue HKCU "Software\RegisteredApplications" "${PRODUCTNAME}"
  DeleteRegKey HKCU "Software\${BUNDLEID}\Capabilities"
  DeleteRegKey /ifempty HKCU "Software\${BUNDLEID}"
  DeleteRegKey HKCU "Software\Classes\${BUNDLEID}.mailto"
  ; Stage 21 (decision 0070): "Start with the system" is the value ${PRODUCTNAME} under the Run
  ; key, which Tauri's own uninstaller removes (not on updates); Task Manager's state of it goes
  ; too. An update keeps both, so the app still starts with the system afterwards.
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "${PRODUCTNAME}"
  ${EndIf}
!macroend

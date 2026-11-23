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
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue HKCU "Software\RegisteredApplications" "${PRODUCTNAME}"
  DeleteRegKey HKCU "Software\${BUNDLEID}\Capabilities"
  DeleteRegKey /ifempty HKCU "Software\${BUNDLEID}"
  DeleteRegKey HKCU "Software\Classes\${BUNDLEID}.mailto"
!macroend

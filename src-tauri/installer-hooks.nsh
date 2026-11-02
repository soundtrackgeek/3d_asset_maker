; Put the install folder (app + t2.exe) on the *user* PATH so agents can run `t2` from any shell.
; The registry edit is done by `t2 path add|remove`, which keeps the value type, backs up the old
; value and refuses to write anything it could not read. NSIS' own ReadRegStr silently returns an
; empty string for PATH values over ~1 KB, so it must never be used to rewrite PATH.

!macro NSIS_HOOK_POSTINSTALL
  nsExec::Exec '"$INSTDIR\t2.exe" path add "$INSTDIR"'
  Pop $0
  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::Exec '"$INSTDIR\t2.exe" path remove "$INSTDIR"'
  Pop $0
  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend

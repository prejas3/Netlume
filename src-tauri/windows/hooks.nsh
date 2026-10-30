; CCS/1 uninstall drops the autostart logon task the tray toggle made.
!macro NSIS_HOOK_PREUNINSTALL
  nsExec::Exec 'schtasks /Delete /TN Netlume /F'
!macroend

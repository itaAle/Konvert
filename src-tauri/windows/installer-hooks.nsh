; Konvert installer hooks, included by Tauri's NSIS template.
;
; 1. "Open with Konvert": Konvert is added to the "Open with" list of the
;    supported formats (OpenWithProgids). Default apps are never changed, so the
;    association stays optional: the user picks Konvert only when they want to.
;    Tauri's built-in `fileAssociations` would make Konvert the default handler,
;    which is why it is not used.
; 2. WebView2 check: the installer never downloads anything. If the WebView2
;    runtime is missing (rare: it ships with Windows 11 and updated Windows 10),
;    the user is told how to install it.
;
; The registry entries go to SHCTX (HKCU for the per-user install) and are
; removed on uninstall. Konvert's preferences are kept on updates; the
; uninstaller removes them only if "Delete the application data" is checked.

!define KONVERT_PROGID "Konvert.Media"
!define KONVERT_WEBVIEW2_KEY "Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"

; Every input extension Konvert accepts in the file picker.
!macro KONVERT_EACH_EXTENSION OP
  !insertmacro ${OP} "mp3"
  !insertmacro ${OP} "m4a"
  !insertmacro ${OP} "m4b"
  !insertmacro ${OP} "aac"
  !insertmacro ${OP} "wav"
  !insertmacro ${OP} "wave"
  !insertmacro ${OP} "flac"
  !insertmacro ${OP} "ogg"
  !insertmacro ${OP} "oga"
  !insertmacro ${OP} "opus"
  !insertmacro ${OP} "wma"
  !insertmacro ${OP} "aif"
  !insertmacro ${OP} "aiff"
  !insertmacro ${OP} "aifc"
  !insertmacro ${OP} "alac"
  !insertmacro ${OP} "ape"
  !insertmacro ${OP} "wv"
  !insertmacro ${OP} "mka"
  !insertmacro ${OP} "ac3"
  !insertmacro ${OP} "eac3"
  !insertmacro ${OP} "mp2"
  !insertmacro ${OP} "caf"
  !insertmacro ${OP} "mp4"
  !insertmacro ${OP} "m4v"
  !insertmacro ${OP} "mkv"
  !insertmacro ${OP} "mov"
  !insertmacro ${OP} "qt"
  !insertmacro ${OP} "webm"
  !insertmacro ${OP} "avi"
  !insertmacro ${OP} "wmv"
  !insertmacro ${OP} "flv"
  !insertmacro ${OP} "mpg"
  !insertmacro ${OP} "mpeg"
  !insertmacro ${OP} "ts"
  !insertmacro ${OP} "m2ts"
  !insertmacro ${OP} "mts"
  !insertmacro ${OP} "3gp"
  !insertmacro ${OP} "ogv"
  !insertmacro ${OP} "vob"
  !insertmacro ${OP} "gif"
  !insertmacro ${OP} "apng"
  !insertmacro ${OP} "jpg"
  !insertmacro ${OP} "jpeg"
  !insertmacro ${OP} "jpe"
  !insertmacro ${OP} "jfif"
  !insertmacro ${OP} "png"
  !insertmacro ${OP} "webp"
  !insertmacro ${OP} "avif"
  !insertmacro ${OP} "heic"
  !insertmacro ${OP} "heif"
  !insertmacro ${OP} "hif"
  !insertmacro ${OP} "bmp"
  !insertmacro ${OP} "tif"
  !insertmacro ${OP} "tiff"
  !insertmacro ${OP} "ico"
!macroend

!macro KONVERT_REGISTER_EXTENSION EXT
  WriteRegStr SHCTX "Software\Classes\.${EXT}\OpenWithProgids" "${KONVERT_PROGID}" ""
  WriteRegStr SHCTX "Software\Classes\Applications\${MAINBINARYNAME}.exe\SupportedTypes" ".${EXT}" ""
!macroend

!macro KONVERT_UNREGISTER_EXTENSION EXT
  ; Only our own value: the key may hold other programs' entries.
  DeleteRegValue SHCTX "Software\Classes\.${EXT}\OpenWithProgids" "${KONVERT_PROGID}"
!macroend

!macro KONVERT_NOTIFY_SHELL
  ; SHCNE_ASSOCCHANGED, SHCNF_FLUSH
  System::Call "shell32::SHChangeNotify(i 0x08000000, i 0x1000, p 0, p 0)"
!macroend

!macro NSIS_HOOK_PREINSTALL
  ReadRegStr $0 HKLM "SOFTWARE\WOW6432Node\${KONVERT_WEBVIEW2_KEY}" "pv"
  ${If} $0 == ""
    ReadRegStr $0 HKLM "SOFTWARE\${KONVERT_WEBVIEW2_KEY}" "pv"
  ${EndIf}
  ${If} $0 == ""
    ReadRegStr $0 HKCU "SOFTWARE\${KONVERT_WEBVIEW2_KEY}" "pv"
  ${EndIf}
  ${If} $0 == ""
    ${If} $LANGUAGE = 1040 ; Italian
      MessageBox MB_ICONINFORMATION|MB_OK "Konvert richiede il componente Microsoft Edge WebView2 Runtime, che non risulta installato.$\n$\nL'installazione continua, ma prima di avviare Konvert installa WebView2 dal sito Microsoft (cerca $\"WebView2 Runtime$\"). Questo installer non scarica nulla da Internet." /SD IDOK
    ${Else}
      MessageBox MB_ICONINFORMATION|MB_OK "Konvert needs the Microsoft Edge WebView2 Runtime, which does not seem to be installed.$\n$\nSetup will continue, but before starting Konvert please install WebView2 from Microsoft's website (search for $\"WebView2 Runtime$\"). This installer does not download anything from the Internet." /SD IDOK
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Program ID used only by "Open with".
  WriteRegStr SHCTX "Software\Classes\${KONVERT_PROGID}" "" "${PRODUCTNAME}"
  WriteRegStr SHCTX "Software\Classes\${KONVERT_PROGID}\DefaultIcon" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\",0"
  WriteRegStr SHCTX "Software\Classes\${KONVERT_PROGID}\shell\open" "FriendlyAppName" "${PRODUCTNAME}"
  WriteRegStr SHCTX "Software\Classes\${KONVERT_PROGID}\shell\open\command" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\""
  WriteRegStr SHCTX "Software\Classes\Applications\${MAINBINARYNAME}.exe" "FriendlyAppName" "${PRODUCTNAME}"
  WriteRegStr SHCTX "Software\Classes\Applications\${MAINBINARYNAME}.exe\shell\open\command" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" $\"%1$\""
  !insertmacro KONVERT_EACH_EXTENSION KONVERT_REGISTER_EXTENSION
  !insertmacro KONVERT_NOTIFY_SHELL
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro KONVERT_EACH_EXTENSION KONVERT_UNREGISTER_EXTENSION
  DeleteRegKey SHCTX "Software\Classes\${KONVERT_PROGID}"
  DeleteRegKey SHCTX "Software\Classes\Applications\${MAINBINARYNAME}.exe"
  !insertmacro KONVERT_NOTIFY_SHELL
!macroend

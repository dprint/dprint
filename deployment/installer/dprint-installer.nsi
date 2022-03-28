# dprint installer script
# Copyright 2020-2022 David Sherret. All rights reserved. MIT license.

Name "dprint"

RequestExecutionLevel User

OutFile "dprint-x86_64-pc-windows-msvc-installer.exe"
InstallDir $PROFILE\.dprint

!macro KillDprintProcess
    # https://stackoverflow.com/a/34371858/188246
    nsExec::ExecToStack `wmic Path win32_process where "name like 'dprint.exe'" Call Terminate`
    Pop $0 # return value
    Pop $1 # printed text
!macroend

Section

    !insertmacro KillDprintProcess

    CreateDirectory $INSTDIR\bin
    SetOutPath $INSTDIR\bin
    File ..\..\target\release\dprint.exe

    nsExec::ExecToStack '"$INSTDIR\bin\dprint" hidden windows-install "$INSTDIR\bin"'
    Pop $0
    Pop $1

    SetOutPath $INSTDIR
    WriteUninstaller $INSTDIR\uninstall.exe

    # Note: Don't bother adding to registry keys in order to do "Add/remove programs"
    # because we'd rather run the installer with `RequestExecutionLevel User`. We
    # tell the user in this message how to uninstall if they wish to do so.

    MessageBox MB_OK "Success! Installed to: $INSTDIR$\n$\nTo get started, restart your terminal and \
        run the following command:$\n$\n    dprint --help$\n$\nTo uninstall run: $INSTDIR\uninstall.exe"

SectionEnd

Section "Uninstall"

    nsExec::ExecToStack '"$INSTDIR\bin\dprint" hidden windows-uninstall "$INSTDIR\bin"'
    Pop $0
    Pop $1

    !insertmacro KillDprintProcess

    Delete $INSTDIR\uninstall.exe
    Delete $INSTDIR\bin\dprint.exe
    RMDir $INSTDIR\bin
    RMDir $INSTDIR

    # delete the plugin cache folder
    RMDir /r $LOCALAPPDATA\Dprint

SectionEnd

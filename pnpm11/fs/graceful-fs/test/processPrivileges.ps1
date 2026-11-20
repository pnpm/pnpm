param([int]$TargetProcessId, [string]$RestoreState)

$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

public static class ProcessPrivileges {
    [DllImport("advapi32.dll", SetLastError = true)]
    static extern bool OpenProcessToken(IntPtr process, int access, out SafeAccessTokenHandle token);

    [DllImport("advapi32.dll", SetLastError = true)]
    static extern bool GetTokenInformation(SafeAccessTokenHandle token, int informationClass,
        [Out] byte[] information, int length, out int requiredLength);

    [DllImport("advapi32.dll", SetLastError = true)]
    static extern bool AdjustTokenPrivileges(SafeAccessTokenHandle token, bool disableAll,
        byte[] newState, int length, IntPtr previousState, IntPtr requiredLength);

    public static string Set(int processId, string restoreState) {
        using (var process = Process.GetProcessById(processId)) {
            SafeAccessTokenHandle token;
            const int TokenQueryAndAdjustPrivileges = 0x28;
            if (!OpenProcessToken(process.Handle, TokenQueryAndAdjustPrivileges, out token))
                throw new Win32Exception();
            using (token) {
                byte[] state;
                bool disableAll = String.IsNullOrEmpty(restoreState);
                if (disableAll) {
                    const int TokenPrivileges = 3;
                    int length;
                    GetTokenInformation(token, TokenPrivileges, null, 0, out length);
                    const int ErrorInsufficientBuffer = 122;
                    if (Marshal.GetLastWin32Error() != ErrorInsufficientBuffer)
                        throw new Win32Exception();
                    state = new byte[length];
                    if (!GetTokenInformation(token, TokenPrivileges, state, state.Length, out length))
                        throw new Win32Exception();
                } else {
                    state = Convert.FromBase64String(restoreState);
                }
                if (!AdjustTokenPrivileges(token, disableAll, state, 0, IntPtr.Zero, IntPtr.Zero)
                    || Marshal.GetLastWin32Error() != 0)
                    throw new Win32Exception();
                return Convert.ToBase64String(state);
            }
        }
    }
}
'@

[ProcessPrivileges]::Set($TargetProcessId, $RestoreState)

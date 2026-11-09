# Get-KbHolders <path>: the processes that hold a file open, as "name (pid N)" strings (card #134).
# It asks the Windows Restart Manager, so it finds any process with the file open, not only one
# running it as its exe. Empty when nothing holds it or the lookup fails: it never throws.
#
# One copy, two users: scripts\install.ps1 dot-sources this file, and Restart now's helper
# (src-tauri\src\platform\windows.rs) embeds it with include_str!. Keep it Windows PowerShell 5.1.

function Get-KbHolders([string]$Path) {
    try {
        if (-not ('KanbanRm' -as [type])) {
            Add-Type -ErrorAction Stop -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class KanbanRm {
    [StructLayout(LayoutKind.Sequential)]
    struct UNIQUE_PROCESS { public int Pid; public System.Runtime.InteropServices.ComTypes.FILETIME Start; }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct PROCESS_INFO {
        public UNIQUE_PROCESS Process;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 256)] public string AppName;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 64)] public string ServiceName;
        public int AppType; public uint AppStatus; public uint TSSessionId;
        [MarshalAs(UnmanagedType.Bool)] public bool Restartable;
    }
    [DllImport("rstrtmgr.dll", CharSet = CharSet.Unicode)]
    static extern int RmStartSession(out uint handle, int flags, string key);
    [DllImport("rstrtmgr.dll")]
    static extern int RmEndSession(uint handle);
    [DllImport("rstrtmgr.dll", CharSet = CharSet.Unicode)]
    static extern int RmRegisterResources(uint handle, uint nFiles, string[] files, uint nApps, IntPtr apps, uint nServices, string[] services);
    [DllImport("rstrtmgr.dll")]
    static extern int RmGetList(uint handle, out uint needed, ref uint count, [In, Out] PROCESS_INFO[] info, ref uint reasons);
    public static int[] Pids(string path) {
        uint h; var pids = new List<int>();
        if (RmStartSession(out h, 0, Guid.NewGuid().ToString()) != 0) return pids.ToArray();
        try {
            if (RmRegisterResources(h, 1, new[] { path }, 0, IntPtr.Zero, 0, null) != 0) return pids.ToArray();
            uint needed = 0, count = 0, reasons = 0;
            PROCESS_INFO[] info = null;
            // ERROR_MORE_DATA (234): ask again with room for them all; a few times, as a new
            // holder may start between the two calls.
            for (int tries = 0; tries < 5; tries++) {
                int r = RmGetList(h, out needed, ref count, info, ref reasons);
                if (r == 0) {
                    for (int i = 0; i < count; i++) pids.Add(info[i].Process.Pid);
                    break;
                }
                if (r != 234) break;
                info = new PROCESS_INFO[needed + 2]; count = (uint)info.Length;
            }
        } finally { RmEndSession(h); }
        return pids.ToArray();
    }
}
'@
        }
        foreach ($id in [KanbanRm]::Pids($Path)) {
            $p = Get-Process -Id $id -ErrorAction SilentlyContinue
            $name = if ($p) { "$($p.ProcessName).exe" } else { 'a process' }
            "$name (pid $id)"
        }
    } catch {
        # A lookup that fails is no reason to fail an install: the caller says "unknown".
    }
}

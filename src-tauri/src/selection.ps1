$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, WindowsBase
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class HaloWin {
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
}

// Talks to the desktop's own SysListView32 directly (ScreenToClient + LVM_HITTEST / LVM_GETNEXTITEM / LVM_GETITEMTEXT).
// A screen-wide overlay (e.g. a GPU overlay widget) can sit above the desktop in the UI Automation hit-test order and
// make AutomationElement.FromPoint always resolve to it instead of the desktop, so that API cannot be trusted here.
public static class DesktopList {
    [DllImport("user32.dll", CharSet=CharSet.Auto)] static extern IntPtr FindWindow(string cls, string name);
    [DllImport("user32.dll", CharSet=CharSet.Auto)] static extern IntPtr FindWindowEx(IntPtr parent, IntPtr after, string cls, string name);
    [DllImport("user32.dll")] static extern bool ScreenToClient(IntPtr hWnd, ref POINT lpPoint);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint procId);
    [DllImport("user32.dll")] static extern IntPtr SendMessage(IntPtr hWnd, int Msg, IntPtr wParam, IntPtr lParam);
    [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll")] static extern IntPtr VirtualAllocEx(IntPtr hProc, IntPtr addr, uint size, uint allocType, uint protect);
    [DllImport("kernel32.dll")] static extern bool VirtualFreeEx(IntPtr hProc, IntPtr addr, uint size, uint freeType);
    [DllImport("kernel32.dll")] static extern bool WriteProcessMemory(IntPtr hProc, IntPtr addr, byte[] buffer, int size, out int written);
    [DllImport("kernel32.dll")] static extern bool ReadProcessMemory(IntPtr hProc, IntPtr addr, byte[] buffer, int size, out int read);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);

    [StructLayout(LayoutKind.Sequential)] struct POINT { public int X; public int Y; }
    [StructLayout(LayoutKind.Sequential)] struct LVHITTESTINFO { public POINT pt; public uint flags; public int iItem; public int iSubItem; public int iGroup; }
    [StructLayout(LayoutKind.Sequential)]
    struct LVITEM {
        public uint mask; public int iItem; public int iSubItem; public uint state; public uint stateMask;
        public IntPtr pszText; public int cchTextMax; public int iImage; public IntPtr lParam;
        public int iIndent; public int iGroupId; public uint cColumns; public IntPtr puColumns; public IntPtr piColFmt; public int iGroup2;
    }

    const int LVM_HITTEST = 0x1012;
    const int LVM_GETNEXTITEM = 0x100C;
    const int LVM_GETITEMTEXTW = 0x1073;
    const uint LVNI_SELECTED = 0x0002;
    const uint PROCESS_ACCESS = 0x0008 | 0x0010 | 0x0020; // VM_OPERATION | VM_READ | VM_WRITE
    const uint MEM_COMMIT = 0x1000;
    const uint MEM_RELEASE = 0x8000;
    const uint PAGE_READWRITE = 0x04;

    static byte[] ToBytes<T>(T s) where T : struct {
        int size = Marshal.SizeOf(typeof(T));
        IntPtr p = Marshal.AllocHGlobal(size);
        Marshal.StructureToPtr(s, p, false);
        byte[] arr = new byte[size];
        Marshal.Copy(p, arr, 0, size);
        Marshal.FreeHGlobal(p);
        return arr;
    }
    static T FromBytes<T>(byte[] arr) where T : struct {
        int size = Marshal.SizeOf(typeof(T));
        IntPtr p = Marshal.AllocHGlobal(size);
        Marshal.Copy(arr, 0, p, size);
        T s = (T)Marshal.PtrToStructure(p, typeof(T));
        Marshal.FreeHGlobal(p);
        return s;
    }

    public static IntPtr Find() {
        IntPtr progman = FindWindow("Progman", null);
        IntPtr defView = FindWindowEx(progman, IntPtr.Zero, "SHELLDLL_DefView", null);
        if (defView == IntPtr.Zero) {
            IntPtr w = IntPtr.Zero;
            while (true) {
                w = FindWindowEx(IntPtr.Zero, w, "WorkerW", null);
                if (w == IntPtr.Zero) break;
                IntPtr dv = FindWindowEx(w, IntPtr.Zero, "SHELLDLL_DefView", null);
                if (dv != IntPtr.Zero) { defView = dv; break; }
            }
        }
        if (defView == IntPtr.Zero) return IntPtr.Zero;
        return FindWindowEx(defView, IntPtr.Zero, "SysListView32", null);
    }

    /// The icon index under a screen point, or -1 if the point isn't on one.
    public static int HitTest(IntPtr listview, int x, int y) {
        uint pid; GetWindowThreadProcessId(listview, out pid);
        IntPtr hProc = OpenProcess(PROCESS_ACCESS, false, pid);
        if (hProc == IntPtr.Zero) return -1;
        try {
            POINT pt = new POINT { X = x, Y = y };
            ScreenToClient(listview, ref pt);
            int size = Marshal.SizeOf(typeof(LVHITTESTINFO));
            IntPtr remote = VirtualAllocEx(hProc, IntPtr.Zero, (uint)size, MEM_COMMIT, PAGE_READWRITE);
            if (remote == IntPtr.Zero) return -1;
            try {
                var hti = new LVHITTESTINFO { pt = pt };
                int written;
                WriteProcessMemory(hProc, remote, ToBytes(hti), size, out written);
                SendMessage(listview, LVM_HITTEST, IntPtr.Zero, remote);
                byte[] outBytes = new byte[size];
                int read;
                ReadProcessMemory(hProc, remote, outBytes, size, out read);
                return FromBytes<LVHITTESTINFO>(outBytes).iItem;
            } finally { VirtualFreeEx(hProc, remote, 0, MEM_RELEASE); }
        } finally { CloseHandle(hProc); }
    }

    /// Every currently selected icon's index.
    public static int[] SelectedIndices(IntPtr listview) {
        var list = new System.Collections.Generic.List<int>();
        int i = -1;
        while (true) {
            IntPtr next = SendMessage(listview, LVM_GETNEXTITEM, (IntPtr)i, (IntPtr)LVNI_SELECTED);
            i = next.ToInt32();
            if (i < 0) break;
            list.Add(i);
        }
        return list.ToArray();
    }

    public static string ItemText(IntPtr listview, int index) {
        uint pid; GetWindowThreadProcessId(listview, out pid);
        IntPtr hProc = OpenProcess(PROCESS_ACCESS, false, pid);
        if (hProc == IntPtr.Zero) return null;
        try {
            int bufBytes = 1024;
            IntPtr remoteText = VirtualAllocEx(hProc, IntPtr.Zero, (uint)bufBytes, MEM_COMMIT, PAGE_READWRITE);
            int lvitemSize = Marshal.SizeOf(typeof(LVITEM));
            IntPtr remoteItem = VirtualAllocEx(hProc, IntPtr.Zero, (uint)lvitemSize, MEM_COMMIT, PAGE_READWRITE);
            if (remoteText == IntPtr.Zero || remoteItem == IntPtr.Zero) return null;
            try {
                var lvi = new LVITEM { iSubItem = 0, cchTextMax = bufBytes / 2, pszText = remoteText };
                int written;
                WriteProcessMemory(hProc, remoteItem, ToBytes(lvi), lvitemSize, out written);
                SendMessage(listview, LVM_GETITEMTEXTW, (IntPtr)index, remoteItem);
                byte[] textBytes = new byte[bufBytes];
                int read;
                ReadProcessMemory(hProc, remoteText, textBytes, bufBytes, out read);
                string s = Encoding.Unicode.GetString(textBytes);
                int nullIdx = s.IndexOf('\0');
                return nullIdx >= 0 ? s.Substring(0, nullIdx) : s;
            } finally {
                VirtualFreeEx(hProc, remoteText, 0, MEM_RELEASE);
                VirtualFreeEx(hProc, remoteItem, 0, MEM_RELEASE);
            }
        } finally { CloseHandle(hProc); }
    }
}
"@
[void][HaloWin]::SetProcessDPIAware()

$shell = New-Object -ComObject Shell.Application
$desktopDirs = @([Environment]::GetFolderPath('Desktop'), [Environment]::GetFolderPath('CommonDesktopDirectory'))
$AE = [Windows.Automation.AutomationElement]
$CT = [Windows.Automation.ControlType]

function Find-ListItem($el) {
    for ($i = 0; $i -lt 5 -and $null -ne $el; $i++) {
        $t = $el.Current.ControlType
        if ($t -eq $CT::ListItem -or $t -eq $CT::DataItem) { return $el }
        $el = [Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($el)
    }
    return $null
}

function Resolve-Desktop($names) {
    $found = @()
    foreach ($n in $names) {
        foreach ($d in $desktopDirs) {
            $m = Get-ChildItem -LiteralPath $d | Where-Object { $_.Name -eq $n -or $_.BaseName -eq $n } | Select-Object -First 1
            if ($m) { $found += $m.FullName; break }
        }
    }
    return $found
}

function Get-Selection($line) {
    $parts = $line.Split(' ')
    $x = [int]$parts[1]; $y = [int]$parts[2]
    $fg = [HaloWin]::GetForegroundWindow()
    $sb = New-Object System.Text.StringBuilder 256
    [void][HaloWin]::GetClassName($fg, $sb, 256)
    $cls = $sb.ToString()

    if ($cls -eq 'CabinetWClass') {
        $item = $null; $name = $null
        if ($x -ge 0) {
            $el = $AE::FromPoint((New-Object Windows.Point $x, $y))
            $item = Find-ListItem $el
            if ($null -eq $item) { return @() }
            $name = $item.Current.Name
        }
        $paths = @()
        foreach ($w in $shell.Windows()) {
            if ([int64]$w.HWND -eq $fg.ToInt64()) {
                foreach ($i in $w.Document.SelectedItems()) {
                    if (Test-Path -LiteralPath $i.Path) { $paths += $i.Path }
                }
            }
        }
        if ($null -ne $name) {
            $hit = $false
            foreach ($p in $paths) {
                if ([IO.Path]::GetFileName($p) -eq $name -or [IO.Path]::GetFileNameWithoutExtension($p) -eq $name) { $hit = $true }
            }
            if (-not $hit) { return @() }
        }
        return $paths
    } elseif ($cls -eq 'Progman' -or $cls -eq 'WorkerW') {
        # UI Automation's point hit-testing can't be trusted here: a screen-wide overlay (e.g. a GPU
        # overlay widget) sits above the desktop in its hit-test order, so talk to the real desktop
        # ListView control directly instead (ScreenToClient + LVM_HITTEST / LVM_GETNEXTITEM).
        $listview = [DesktopList]::Find()
        if ($listview -eq [IntPtr]::Zero) { return @() }
        if ($x -ge 0) {
            $hitIndex = [DesktopList]::HitTest($listview, $x, $y)
            if ($hitIndex -lt 0) { return @() }
        }
        $indices = [DesktopList]::SelectedIndices($listview)
        if ($indices.Count -eq 0 -and $x -ge 0) { $indices = @($hitIndex) }
        $names = @($indices | ForEach-Object { [DesktopList]::ItemText($listview, $_) } | Where-Object { $_ })
        return @(Resolve-Desktop $names)
    }
    return @()
}

[Console]::Out.WriteLine('ready'); [Console]::Out.Flush()
while ($true) {
    $line = [Console]::In.ReadLine()
    if ($null -eq $line) { break }
    $out = @()
    try { $out = @(Get-Selection $line) } catch {}
    [Console]::Out.WriteLine((ConvertTo-Json -InputObject @($out) -Compress))
    [Console]::Out.Flush()
}

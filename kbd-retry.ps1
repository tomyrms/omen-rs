$ErrorActionPreference = 'Continue'
$out = 'C:\Users\jtoma\omen-rs\live-test-result.txt'
$lines = New-Object System.Collections.Generic.List[string]

Add-Type -Namespace K7 -Name N -MemberDefinition @'
[DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
public static extern IntPtr CreateFileW(string name, uint access, uint share, IntPtr sec, uint disp, uint flags, IntPtr tmpl);
[DllImport("kernel32.dll", SetLastError=true)]
public static extern bool WriteFile(IntPtr h, byte[] buf, uint n, out uint written, IntPtr ov);
[DllImport("kernel32.dll", SetLastError=true)]
public static extern bool CloseHandle(IntPtr h);
'@

$guid = '{4d1e55b2-f16f-11cf-88cb-001111000030}'
$root = "Registry::HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\DeviceClasses\$guid"
$path = $null
foreach ($k in (Get-ChildItem $root -ErrorAction SilentlyContinue)) {
    if ($k.PSChildName -match 'VID_0461&PID_4E9A&MI_02') { $path = '\\?\' + $k.PSChildName.Substring(4); break }
}
if (-not $path) { $lines.Add('NO DEVICE'); $lines -join "`r`n" | Out-File $out -Encoding utf8; exit 1 }
$h = [K7.N]::CreateFileW($path, [uint32]3221225472, [uint32]3, [IntPtr]::Zero, [uint32]3, [uint32]0, [IntPtr]::Zero)

function New-Report([int]$c1, [int]$c2, [int]$len, [byte[]]$data) {
    $r = New-Object byte[] 65
    $r[1] = [byte]$c1; $r[2] = [byte]$c2; $r[3] = [byte]$len; $r[4] = 0
    if ($data) { [Array]::Copy($data, 0, $r, 5, [Math]::Min($data.Length, 60)) }
    return $r
}
function Send([byte[]]$rep, [string]$label) {
    $w = [uint32]0
    $ok = [K7.N]::WriteFile($h, $rep, [uint32]65, [ref]$w, [IntPtr]::Zero)
    if (-not ($ok -and $w -eq 65)) { $lines.Add("  FAIL $label") }
}
function Push-Magenta([string]$tag) {
    Send (New-Report 4 0 2 @(0xFC, 0xEA)) 'user-mode'
    Send (New-Report 9 0 1 @(0xFF)) 'lighting-on'
    $d = New-Object byte[] 60
    $d[3] = 255; $d[4] = 0; $d[5] = 255
    Send (New-Report 3 1 22 $d) 'effect-magenta'
    $km = New-Object byte[] 60
    foreach ($page in 0, 1, 2) {
        for ($i = 0; $i -lt 60; $i++) { $km[$i] = 255 }
        Send (New-Report 5 $page 60 $km) "R p$page"
        for ($i = 0; $i -lt 60; $i++) { $km[$i] = 0 }
        Send (New-Report 6 $page 60 $km) "G p$page"
        Send (New-Report 7 $page 60 $km) "B p$page"
    }
    $lines.Add("magenta pushed ($tag)")
}

for ($round = 0; $round -lt 6; $round++) {
    $procs = Get-Process -Name OmenCommandCenterBackground, OmenCap -ErrorAction SilentlyContinue
    if ($procs) {
        try { $procs | Stop-Process -Force -ErrorAction Stop; $lines.Add("killed writers (round $round)") } catch { $lines.Add("kill failed: $($_.Exception.Message)") }
    }
    Push-Magenta "round $round"
    Start-Sleep -Seconds 2
}

[K7.N]::CloseHandle($h) | Out-Null
$lines.Add('done - 12s de magenta pousse')
$lines -join "`r`n" | Out-File $out -Encoding utf8
"done"

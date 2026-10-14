$ErrorActionPreference = 'Continue'
$out = 'C:\Users\jtoma\omen-rs\live-test-result.txt'
$lines = New-Object System.Collections.Generic.List[string]

Add-Type -Namespace K6 -Name N -MemberDefinition @'
[DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
public static extern IntPtr CreateFileW(string name, uint access, uint share, IntPtr sec, uint disp, uint flags, IntPtr tmpl);
[DllImport("kernel32.dll", SetLastError=true)]
public static extern bool WriteFile(IntPtr h, byte[] buf, uint n, out uint written, IntPtr ov);
[DllImport("kernel32.dll", SetLastError=true)]
public static extern bool CloseHandle(IntPtr h);
'@

# --- stop the app-side lighting/animation writers ---
foreach ($proc in 'OmenCommandCenterBackground', 'OmenCap') {
    $p = Get-Process -Name $proc -ErrorAction SilentlyContinue
    if ($p) {
        try { $p | Stop-Process -Force -ErrorAction Stop; $lines.Add("stopped process $proc") }
        catch { $lines.Add("cannot stop ${proc}: $($_.Exception.Message)") }
    } else { $lines.Add("process $proc not running") }
}
try {
    Stop-Service -Name HPOmenCap -Force -ErrorAction Stop
    $lines.Add('stopped service HPOmenCap')
} catch { $lines.Add("service stop failed: $($_.Exception.Message)") }
Start-Sleep -Seconds 2

# --- open device and push MAGENTA static ---
$guid = '{4d1e55b2-f16f-11cf-88cb-001111000030}'
$root = "Registry::HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\DeviceClasses\$guid"
$path = $null
foreach ($k in (Get-ChildItem $root -ErrorAction SilentlyContinue)) {
    if ($k.PSChildName -match 'VID_0461&PID_4E9A&MI_02') { $path = '\\?\' + $k.PSChildName.Substring(4); break }
}
if (-not $path) { $lines.Add('NO DEVICE'); $lines -join "`r`n" | Out-File $out -Encoding utf8; exit 1 }
$h = [K6.N]::CreateFileW($path, [uint32]3221225472, [uint32]3, [IntPtr]::Zero, [uint32]3, [uint32]0, [IntPtr]::Zero)

function New-Report([int]$c1, [int]$c2, [int]$len, [byte[]]$data) {
    $r = New-Object byte[] 65
    $r[1] = [byte]$c1; $r[2] = [byte]$c2; $r[3] = [byte]$len; $r[4] = 0
    if ($data) { [Array]::Copy($data, 0, $r, 5, [Math]::Min($data.Length, 60)) }
    return $r
}
function Send([byte[]]$rep, [string]$label) {
    $w = [uint32]0
    $ok = [K6.N]::WriteFile($h, $rep, [uint32]65, [ref]$w, [IntPtr]::Zero)
    $lines.Add("  $(if ($ok -and $w -eq 65) { 'OK ' } else { 'FAIL' }) $label")
    Start-Sleep -Milliseconds 150
}

$lines.Add('--- sending MAGENTA static ---')
Send (New-Report 4 0 2 @(0xFC, 0xEA)) 'user-mode enable'
Send (New-Report 9 0 1 @(0xFF)) 'lighting ON'
$data = New-Object byte[] 60
$data[3] = 255; $data[4] = 0; $data[5] = 255
Send (New-Report 3 1 22 $data) 'V1 effect E=1 MAGENTA'
$km = New-Object byte[] 60
foreach ($page in 0, 1, 2) {
    for ($i = 0; $i -lt 60; $i++) { $km[$i] = 255 }
    Send (New-Report 5 $page 60 $km) "R p$page (255)"
    for ($i = 0; $i -lt 60; $i++) { $km[$i] = 0 }
    Send (New-Report 6 $page 60 $km) "G p$page (0)"
    Send (New-Report 7 $page 60 $km) "B p$page (255)"
    for ($i = 0; $i -lt 60; $i++) { $km[$i] = 255 }
    $km[0] = 255
}
[K6.N]::CloseHandle($h) | Out-Null
$lines.Add('done - regarde le clavier maintenant')
$lines -join "`r`n" | Out-File $out -Encoding utf8
"done"

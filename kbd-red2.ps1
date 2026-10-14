$ErrorActionPreference = 'Continue'
$out = 'C:\Users\jtoma\omen-rs\live-kbd-red2.txt'
$lines = [System.Collections.Generic.List[string]]::new()
$lines.Add("== kbd RED test v2 (service running, no kills) $(Get-Date -Format 'HH:mm:ss') ==")

Add-Type -Namespace K6 -Name N -MemberDefinition @'
[DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] public static extern IntPtr CreateFileW(string name, uint access, uint share, IntPtr sec, uint disp, uint flags, IntPtr tmpl);
[DllImport("kernel32.dll", SetLastError=true)] public static extern bool WriteFile(IntPtr h, byte[] buf, uint n, out uint written, IntPtr ov);
[DllImport("kernel32.dll", SetLastError=true)] public static extern bool CloseHandle(IntPtr h);
'@

$guid = '{4d1e55b2-f16f-11cf-88cb-001111000030}'
$root = "Registry::HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\DeviceClasses\$guid"
$path = $null
foreach ($k in (Get-ChildItem $root -ErrorAction SilentlyContinue)) {
    if ($k.PSChildName -match 'VID_0461&PID_4E9A&MI_02') { $path = '\\?\' + $k.PSChildName.Substring(4); break }
}
if (-not $path) { $lines.Add('no device'); $lines -join "`r`n" | Out-File $out -Encoding utf8; 'no device'; exit 1 }

$h = [K6.N]::CreateFileW($path, [uint32]3221225472, [uint32]3, [IntPtr]::Zero, [uint32]3, [uint32]0, [IntPtr]::Zero)
if ($h.ToInt64() -eq -1 -or $h.ToInt64() -eq 0) {
    $err = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
    $lines.Add("open failed: $err")
    $lines -join "`r`n" | Out-File $out -Encoding utf8; "open failed $err"; exit 1
}
$lines.Add("path=$path")

function Send([int]$c1, [int]$c2, [int]$len, [byte[]]$data, [string]$label) {
    $r = New-Object byte[] 65
    $r[1] = [byte]$c1; $r[2] = [byte]$c2; $r[3] = [byte]$len; $r[4] = 0
    if ($data) { [Array]::Copy($data, 0, $r, 5, [Math]::Min($data.Length, 60)) }
    $w = [uint32]0
    $ok = [K6.N]::WriteFile($h, $r, [uint32]65, [ref]$w, [IntPtr]::Zero)
    $werr = if (-not $ok) { [Runtime.InteropServices.Marshal]::GetLastWin32Error() } else { 0 }
    $lines.Add("  $(if ($ok) { 'W-OK ' } else { "W-FAIL(e$werr)" }) $label")
}

function Set-Red() {
    Send 9 0 1 @([byte]0xFF) '1.lighting-on'
    Start-Sleep -Milliseconds 70
    Send 4 0 2 @([byte]0xFC, [byte]0xEA) '2.user-mode-enable'
    Start-Sleep -Milliseconds 70
    $ff = New-Object byte[] 18
    for ($i = 0; $i -lt 18; $i++) { $ff[$i] = 0xFF }
    Send 4 1 18 $ff '3.all-keys-can-change'
    Start-Sleep -Milliseconds 70
    foreach ($page in 0, 1, 2) {
        $dr = New-Object byte[] 60; for ($i = 0; $i -lt 60; $i++) { $dr[$i] = 255 }; Send 5 $page 60 $dr "4.R page$page"
        $dg = New-Object byte[] 60; for ($i = 0; $i -lt 60; $i++) { $dg[$i] = 0 }; Send 6 $page 60 $dg "4.G page$page"
        $db = New-Object byte[] 60; for ($i = 0; $i -lt 60; $i++) { $db[$i] = 0 }; Send 7 $page 60 $db "4.B page$page"
        Start-Sleep -Milliseconds 50
    }
    Start-Sleep -Milliseconds 70
    Send 4 1 18 $ff '5.all-key-can-change-2'
}

Set-Red
[K6.N]::CloseHandle($h) | Out-Null
$lines.Add('done')
$lines -join "`r`n" | Out-File $out -Encoding utf8
'done'

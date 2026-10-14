param([switch]$DryRun)

$ErrorActionPreference = 'Continue'
$out = 'C:\Users\jtoma\omen-rs\live-test-result.txt'
$lines = New-Object System.Collections.Generic.List[string]

Add-Type -Namespace K -Name N -MemberDefinition @'
[DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
public static extern IntPtr CreateFileW(string name, uint access, uint share, IntPtr sec, uint disp, uint flags, IntPtr tmpl);
[DllImport("kernel32.dll", SetLastError=true)]
public static extern bool WriteFile(IntPtr h, byte[] buf, uint n, out uint written, IntPtr ov);
[DllImport("kernel32.dll", SetLastError=true)]
public static extern bool CloseHandle(IntPtr h);
'@

function Find-DevicePath {
    $guid = '{4d1e55b2-f16f-11cf-88cb-001111000030}'
    $root = "Registry::HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\DeviceClasses\$guid"
    $keys = Get-ChildItem $root -ErrorAction SilentlyContinue
    foreach ($k in $keys) {
        if ($k.PSChildName -match 'VID_0461&PID_4E9A&MI_02') {
            return '\\?\' + $k.PSChildName.Substring(4)
        }
    }
    return $null
}

$path = Find-DevicePath
$lines.Add("device path: $path")

Remove-Item $out -ErrorAction SilentlyContinue
if (-not $path) { $lines -join "`r`n" | Out-File $out -Encoding utf8; "no device"; exit 1 }

# 3221225472 = 0xC0000000 = GENERIC_READ | GENERIC_WRITE (must be passed as decimal:
# PowerShell parses 0xC0000000 as a negative Int32)
$h = [K.N]::CreateFileW($path, [uint32]3221225472, [uint32]3, [IntPtr]::Zero, [uint32]3, [uint32]0, [IntPtr]::Zero)
$hv = $h.ToInt64()
if ($hv -eq 0 -or $hv -eq -1) {
    $err = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
    $lines.Add("CreateFile FAILED err=$err")
    $lines -join "`r`n" | Out-File $out -Encoding utf8
    "open failed: $err"; exit 1
}
$lines.Add("device open OK (handle $hv)")
$lines -join "`r`n" | Out-File $out -Encoding utf8

function New-Report([int]$c1, [int]$c2, [int]$len, [byte[]]$data) {
    $r = New-Object byte[] 65
    $r[0] = 0
    $r[1] = [byte]$c1
    $r[2] = [byte]$c2
    $r[3] = [byte]$len
    $r[4] = 0
    if ($data) { [Array]::Copy($data, 0, $r, 5, [Math]::Min($data.Length, 60)) }
    return $r
}

function Send-Report([byte[]]$rep, [string]$label) {
    $written = [uint32]0
    $ok = [K.N]::WriteFile($h, $rep, [uint32]65, [ref]$written, [IntPtr]::Zero)
    if ($ok -and $written -eq 65) { $lines.Add("  OK   $label") }
    else { $lines.Add("  FAIL $label err=$([Runtime.InteropServices.Marshal]::GetLastWin32Error()) written=$written") }
}

function Send-StaticLighting([int]$r, [int]$g, [int]$b, [string]$tag) {
    foreach ($page in 0, 1, 2) {
        $data = New-Object byte[] 60
        for ($i = 0; $i -lt 60; $i++) { $data[$i] = [byte]$r }
        Send-Report (New-Report 5 $page 60 $data) "$tag R page$page"
        for ($i = 0; $i -lt 60; $i++) { $data[$i] = [byte]$g }
        Send-Report (New-Report 6 $page 60 $data) "$tag G page$page"
        for ($i = 0; $i -lt 60; $i++) { $data[$i] = [byte]$b }
        Send-Report (New-Report 7 $page 60 $data) "$tag B page$page"
    }
}

function Send-LightingEffect([int]$effect, [int]$showMode, [int]$r, [int]$g, [int]$b, [string]$tag) {
    $data = New-Object byte[] 60
    $data[0] = [byte]$effect      # Raw[5]
    $data[1] = [byte]$showMode    # Raw[6]
    $data[2] = 0                  # Raw[7] ColorNumber
    $data[3] = [byte]$r           # Raw[8]  R0
    $data[4] = [byte]$g           # Raw[9]  G0
    $data[5] = [byte]$b           # Raw[10] B0
    Send-Report (New-Report 3 0 20 $data) "$tag effect=$effect show=$showMode rgb=$r,$g,$b"
}

function Send-AllKeysOn([string]$tag) {
    $data = New-Object byte[] 60
    for ($i = 0; $i -lt 60; $i++) { $data[$i] = 0xFF }
    Send-Report (New-Report 4 1 18 $data) "$tag individual-onoff all ON"
}

# ---------- experiments ----------
if ($DryRun) {
    $lines.Add("DRY RUN: device opened, skipping writes")
    [K.N]::CloseHandle($h) | Out-Null
    $lines -join "`r`n" | Out-File $out -Encoding utf8
    "dry run ok"
    exit 0
}

$lines.Add("=== T1: user mode + single-color RED via 0x03 ===")
Send-Report (New-Report 4 0 2 @(0xFC, 0xEA)) "user mode enable"
Send-LightingEffect 1 0 255 0 0 "T1"
Start-Sleep -Seconds 7

$lines.Add("=== T2: static keymaps RED (0x05/6/7) ===")
Send-StaticLighting 255 0 0 "T2"
Start-Sleep -Seconds 7

$lines.Add("=== T3: single-color GREEN + keymaps GREEN ===")
Send-LightingEffect 1 0 0 255 0 "T3"
Send-StaticLighting 0 255 0 "T3"
Start-Sleep -Seconds 7

$lines.Add("=== T4: all keys ON bitmap (blue) ===")
Send-LightingEffect 1 0 0 0 255 "T4"
Send-AllKeysOn "T4"
Send-StaticLighting 0 0 255 "T4"
Start-Sleep -Seconds 7

$lines.Add("=== T5: restore default ===")
Send-Report (New-Report 16 7 4 @(0x94, 0x10, 0x98, 0x27)) "restore-to-default"

[K.N]::CloseHandle($h) | Out-Null
$lines.Add("done")
$lines -join "`r`n" | Out-File $out -Encoding utf8
"finished"

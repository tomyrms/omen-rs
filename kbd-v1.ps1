$ErrorActionPreference = 'Continue'
$out = 'C:\Users\jtoma\omen-rs\live-test-result.txt'
$lines = New-Object System.Collections.Generic.List[string]

Add-Type -Namespace K4 -Name N -MemberDefinition @'
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
if (-not $path) { "no device"; exit 1 }
$h = [K4.N]::CreateFileW($path, [uint32]3221225472, [uint32]3, [IntPtr]::Zero, [uint32]3, [uint32]0, [IntPtr]::Zero)
if ($h.ToInt64() -eq -1 -or $h.ToInt64() -eq 0) { "open failed"; exit 1 }

function New-Report([int]$c1, [int]$c2, [int]$len, [byte[]]$data) {
    $r = New-Object byte[] 65
    $r[1] = [byte]$c1; $r[2] = [byte]$c2; $r[3] = [byte]$len; $r[4] = 0
    if ($data) { [Array]::Copy($data, 0, $r, 5, [Math]::Min($data.Length, 60)) }
    return $r
}

function Send([byte[]]$rep, [string]$label) {
    $w = [uint32]0
    $ok = [K4.N]::WriteFile($h, $rep, [uint32]65, [ref]$w, [IntPtr]::Zero)
    $lines.Add("  $(if ($ok -and $w -eq 65) { 'OK ' } else { 'FAIL' }) $label")
}

function Send-Keymaps([int]$r, [int]$g, [int]$b) {
    $data = New-Object byte[] 60
    foreach ($page in 0, 1, 2) {
        for ($i = 0; $i -lt 60; $i++) { $data[$i] = [byte]$r }
        Send (New-Report 5 $page 60 $data) "keymaps R page$page"
        for ($i = 0; $i -lt 60; $i++) { $data[$i] = [byte]$g }
        Send (New-Report 6 $page 60 $data) "keymaps G page$page"
        for ($i = 0; $i -lt 60; $i++) { $data[$i] = [byte]$b }
        Send (New-Report 7 $page 60 $data) "keymaps B page$page"
    }
}

# V1 SetLightingEffect: Raw[2]=Effect, Raw[3]=22, Raw[5]=AssigningColorIndex,
# Raw[6]=ShowMode, Raw[7]=ColorNumber, colors at Raw[8..22], speed/intensity/etc
function Send-V1Effect([int]$effect, [int]$showMode, [int]$colorNumber, [int]$r, [int]$g, [int]$b, [string]$tag) {
    $data = New-Object byte[] 60
    $data[0] = 0            # Raw[5] AssigningColorIndex
    $data[1] = [byte]$showMode   # Raw[6]
    $data[2] = [byte]$colorNumber
    $data[3] = [byte]$r          # R0
    $data[4] = [byte]$g          # G0
    $data[5] = [byte]$b          # B0
    Send (New-Report 3 $effect 22 $data) "$tag V1effect E=$effect show=$showMode num=$colorNumber rgb=$r,$g,$b"
}

$lines.Add('=== A: user mode + lighting ON + keymaps MAGENTA ===')
Send (New-Report 4 0 2 @(0xFC, 0xEA)) 'user-mode enable (FC EA)'
Send (New-Report 9 0 1 @(0xFF)) 'lighting ON (09 FF)'
Send-Keymaps 255 0 255
Start-Sleep -Seconds 7

$lines.Add('=== B: + all keys changeable + keymaps MAGENTA ===')
$ff18 = New-Object byte[] 18
for ($i = 0; $i -lt 18; $i++) { $ff18[$i] = 0xFF }
Send (New-Report 4 1 18 $ff18) 'all-keys-can-change (4/1/18 x FF)'
Send-Keymaps 255 0 255
Start-Sleep -Seconds 7

$lines.Add('=== C: V1 effect E=0 ORANGE ===')
Send-V1Effect 0 0 0 255 140 0 'C'
Start-Sleep -Seconds 7

$lines.Add('=== D: V1 effect E=1 CYAN ===')
Send-V1Effect 1 0 0 0 255 255 'D'
Start-Sleep -Seconds 7

$lines.Add('=== E: V1 effect E=2 WHITE ===')
Send-V1Effect 2 0 0 255 255 255 'E'
Start-Sleep -Seconds 7

$lines.Add('=== F: V1 effect E=3 (rainbow attendu) ===')
Send-V1Effect 3 0 0 255 0 0 'F'
Start-Sleep -Seconds 5

$lines.Add('=== G: restore default ===')
Send (New-Report 16 7 4 @(0x94, 0x10, 0x98, 0x27)) 'restore-to-default'

[K4.N]::CloseHandle($h) | Out-Null
$lines -join "`r`n" | Out-File $out -Encoding utf8
"done"

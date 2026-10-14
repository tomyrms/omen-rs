$ErrorActionPreference = 'Continue'
$out = 'C:\Users\jtoma\omen-rs\live-test-result.txt'
$replyLog = 'C:\Users\jtoma\AppData\Local\Temp\opencode\omen-rea\kbd-replies.txt'
$stopFlag = "$replyLog.stop"
Remove-Item $replyLog, $stopFlag -ErrorAction SilentlyContinue
New-Item -ItemType File -Path $replyLog -Force | Out-Null

Add-Type -Namespace K5 -Name N -MemberDefinition @'
[DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
public static extern IntPtr CreateFileW(string name, uint access, uint share, IntPtr sec, uint disp, uint flags, IntPtr tmpl);
[DllImport("kernel32.dll", SetLastError=true)]
public static extern bool WriteFile(IntPtr h, byte[] buf, uint n, out uint written, IntPtr ov);
[DllImport("kernel32.dll", SetLastError=true)]
public static extern bool ReadFile(IntPtr h, byte[] buf, uint n, out uint read, IntPtr ov);
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
$hW = [K5.N]::CreateFileW($path, [uint32]1073741824, [uint32]3, [IntPtr]::Zero, [uint32]3, [uint32]0, [IntPtr]::Zero)
$hR = [K5.N]::CreateFileW($path, [uint32]2147483648, [uint32]3, [IntPtr]::Zero, [uint32]3, [uint32]0, [IntPtr]::Zero)

# background reader: drains MCU replies
$reader = Start-ThreadJob -ScriptBlock {
    param($h, $logFile, $stop)
    $buf = New-Object byte[] 65
    while (-not (Test-Path $stop)) {
        $read = [uint32]0
        $ok = [K5.N]::ReadFile($h, $buf, [uint32]65, [ref]$read, [IntPtr]::Zero)
        if ($ok -and $read -gt 0) {
            $count = [int]$read
            $hex = ($buf[0..($count - 1)] | ForEach-Object { $_.ToString('X2') }) -join ' '
            Add-Content -Path $logFile -Value $hex
        } else {
            Start-Sleep -Milliseconds 50
        }
    }
} -ArgumentList $hR, $replyLog, $stopFlag

function New-Report([int]$c1, [int]$c2, [int]$len, [byte[]]$data) {
    $r = New-Object byte[] 65
    $r[1] = [byte]$c1; $r[2] = [byte]$c2; $r[3] = [byte]$len; $r[4] = 0
    if ($data) { [Array]::Copy($data, 0, $r, 5, [Math]::Min($data.Length, 60)) }
    return $r
}

function Send([byte[]]$rep, [string]$label) {
    $w = [uint32]0
    $ok = [K5.N]::WriteFile($hW, $rep, [uint32]65, [ref]$w, [IntPtr]::Zero)
    Add-Content -Path $out -Value ("  " + $(if ($ok -and $w -eq 65) { 'OK ' } else { 'FAIL' }) + " " + $label)
    Start-Sleep -Milliseconds 200
}

function Send-Keymaps([int]$r, [int]$g, [int]$b, [string]$tag) {
    $data = New-Object byte[] 60
    foreach ($page in 0, 1, 2) {
        for ($i = 0; $i -lt 60; $i++) { $data[$i] = [byte]$r }
        Send (New-Report 5 $page 60 $data) "$tag R p$page"
        for ($i = 0; $i -lt 60; $i++) { $data[$i] = [byte]$g }
        Send (New-Report 6 $page 60 $data) "$tag G p$page"
        for ($i = 0; $i -lt 60; $i++) { $data[$i] = [byte]$b }
        Send (New-Report 7 $page 60 $data) "$tag B p$page"
    }
}

function Send-V1Effect([int]$effect, [int]$r, [int]$g, [int]$b, [string]$tag) {
    $data = New-Object byte[] 60
    $data[0] = 0; $data[1] = 0; $data[2] = 0
    $data[3] = [byte]$r; $data[4] = [byte]$g; $data[5] = [byte]$b
    Send (New-Report 3 $effect 22 $data) "$tag V1effect E=$effect rgb=$r,$g,$b"
}

Add-Content -Path $out -Value '=== RW test: read after every write ==='
Send (New-Report 0x80 1 12 $null) 'GetDeviceInfo (sanity)'
Start-Sleep -Milliseconds 600
Send (New-Report 4 0 2 @(0xFC, 0xEA)) 'user-mode enable'
Send (New-Report 9 0 1 @(0xFF)) 'lighting ON (09 FF)'
Send-V1Effect 1 255 0 255 'MAGENTA'
Send-Keymaps 255 0 255 'MAGENTA'
Add-Content -Path $out -Value '  --- MAGENTA window open 8s ---'
Start-Sleep -Seconds 8

Send-V1Effect 2 0 255 255 'CYAN'
Send-Keymaps 0 255 255 'CYAN'
Add-Content -Path $out -Value '  --- CYAN window open 8s ---'
Start-Sleep -Seconds 8

Send (New-Report 16 7 4 @(0x94, 0x10, 0x98, 0x27)) 'restore default'

# unblock and stop the reader
Send (New-Report 0x80 1 12 $null) 'GetDeviceInfo (unblock reader)'
Start-Sleep -Milliseconds 800
New-Item -ItemType File -Path $stopFlag -Force | Out-Null
Start-Sleep -Milliseconds 300
$reader | Remove-Job -Force -ErrorAction SilentlyContinue

[K5.N]::CloseHandle($hW) | Out-Null
[K5.N]::CloseHandle($hR) | Out-Null
Add-Content -Path $out -Value 'done'
"done"

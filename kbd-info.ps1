$ErrorActionPreference = 'Continue'
$out = 'C:\Users\jtoma\omen-rs\live-test-result.txt'
$lines = New-Object System.Collections.Generic.List[string]

Add-Type -Namespace K3 -Name N -MemberDefinition @'
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

$hWrite = [K3.N]::CreateFileW($path, [uint32]1073741824, [uint32]3, [IntPtr]::Zero, [uint32]3, [uint32]0, [IntPtr]::Zero)   # GENERIC_WRITE
$hRead  = [K3.N]::CreateFileW($path, [uint32]2147483648, [uint32]3, [IntPtr]::Zero, [uint32]3, [uint32]0, [IntPtr]::Zero)   # GENERIC_READ
$lines.Add("write handle: $($hWrite.ToInt64())  read handle: $($hRead.ToInt64())")

# GetDeviceInfo: cmd 0x80, page 1, len 12
$rep = New-Object byte[] 65
$rep[1] = 0x80; $rep[2] = 1; $rep[3] = 12; $rep[4] = 0
$w = [uint32]0
$ok = [K3.N]::WriteFile($hWrite, $rep, [uint32]65, [ref]$w, [IntPtr]::Zero)
$lines.Add("GetDeviceInfo sent: $ok ($w bytes)")

# blocking read in a thread with timeout
$job = Start-ThreadJob -ScriptBlock {
    param($h)
    $buf = New-Object byte[] 65
    $read = [uint32]0
    $ok = [K3.N]::ReadFile($h, $buf, [uint32]65, [ref]$read, [IntPtr]::Zero)
    [pscustomobject]@{ Ok = $ok; Count = $read; Hex = ($buf | ForEach-Object { $_.ToString('X2') }) -join ' ' }
} -ArgumentList $hRead
$res = $job | Wait-Job -Timeout 4 | Receive-Job
if ($res) {
    $lines.Add("reply ok=$($res.Ok) count=$($res.Count)")
    $lines.Add("raw: $($res.Hex)")
    if ($res.Count -ge 13) {
        $b = $res.Hex -split ' '
        $lines.Add("  firmware  : $($b[6]) $($b[7])")
        $lines.Add("  deviceID  : $($b[8])")
        $lines.Add("  langCode  : $($b[9])")
        $lines.Add("  profileNo : $($b[10])   <- chooses command template")
        $lines.Add("  effectCode: $($b[11])   <- current lighting effect")
        $lines.Add("  brightness: $($b[12])")
    }
} else {
    $lines.Add("no reply within timeout")
}
$job | Remove-Job -Force -ErrorAction SilentlyContinue

[K3.N]::CloseHandle($hWrite) | Out-Null
[K3.N]::CloseHandle($hRead) | Out-Null
$lines -join "`r`n" | Out-File $out -Encoding utf8
"done"

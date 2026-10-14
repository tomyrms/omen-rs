param(
    [ValidateSet('full', 'keymaps', 'effect', 'off', 'default')]
    [string]$Mode = 'full',
    [int]$R = 255,
    [int]$G = 0,
    [int]$B = 0
)

$ErrorActionPreference = 'Continue'
$out = 'C:\Users\jtoma\omen-rs\live-test-result.txt'
$lines = New-Object System.Collections.Generic.List[string]

Add-Type -Namespace K2 -Name N -MemberDefinition @'
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

$h = [K2.N]::CreateFileW($path, [uint32]3221225472, [uint32]3, [IntPtr]::Zero, [uint32]3, [uint32]0, [IntPtr]::Zero)
if ($h.ToInt64() -eq -1 -or $h.ToInt64() -eq 0) { "open failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"; exit 1 }
$lines.Add("mode=$Mode rgb=$R,$G,$B  path=$path")

function New-Report([int]$c1, [int]$c2, [int]$len, [byte[]]$data) {
    $r = New-Object byte[] 65
    $r[1] = [byte]$c1; $r[2] = [byte]$c2; $r[3] = [byte]$len; $r[4] = 0
    if ($data) { [Array]::Copy($data, 0, $r, 5, [Math]::Min($data.Length, 60)) }
    return $r
}

function Send([byte[]]$rep, [string]$label) {
    $w = [uint32]0
    $ok = [K2.N]::WriteFile($h, $rep, [uint32]65, [ref]$w, [IntPtr]::Zero)
    $lines.Add("  $(if ($ok -and $w -eq 65) { 'OK ' } else { 'FAIL' }) $label")
}

function Send-Keymaps([int]$r, [int]$g, [int]$b) {
    $data = New-Object byte[] 60
    foreach ($page in 0, 1, 2) {
        for ($i = 0; $i -lt 60; $i++) { $data[$i] = [byte]$r }
        Send (New-Report 5 $page 60 $data) "R page$page"
        for ($i = 0; $i -lt 60; $i++) { $data[$i] = [byte]$g }
        Send (New-Report 6 $page 60 $data) "G page$page"
        for ($i = 0; $i -lt 60; $i++) { $data[$i] = [byte]$b }
        Send (New-Report 7 $page 60 $data) "B page$page"
    }
}

function Send-Effect([int]$effect, [int]$r, [int]$g, [int]$b) {
    $data = New-Object byte[] 60
    $data[0] = [byte]$effect; $data[1] = 0; $data[2] = 0
    $data[3] = [byte]$r; $data[4] = [byte]$g; $data[5] = [byte]$b
    Send (New-Report 3 0 20 $data) "effect=$effect rgb=$r,$g,$b"
}

switch ($Mode) {
    'full' {
        Send (New-Report 4 0 2 @(0xFC, 0xEA)) 'user mode enable'
        Send-Effect 1 $R $G $B
        Send-Keymaps $R $G $B
    }
    'keymaps' { Send-Keymaps $R $G $B }
    'effect' { Send-Effect 1 $R $G $B }
    'off' { Send-Effect 0 0 0 0 }
    'default' { Send (New-Report 16 7 4 @(0x94, 0x10, 0x98, 0x27)) 'restore-to-default' }
}

[K2.N]::CloseHandle($h) | Out-Null
$lines -join "`r`n" | Out-File $out -Encoding utf8
"done"

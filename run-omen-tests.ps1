$exe = 'C:\Users\jtoma\omen-rs\target\debug\omen.exe'
$out = 'C:\Users\jtoma\omen-rs\live-test-result.txt'
$lines = New-Object System.Collections.Generic.List[string]

function RunStep([string]$title, [string[]]$arguments) {
    $lines.Add("=== $title ===")
    $result = & $exe @arguments 2>&1 | Out-String
    $lines.Add($result.TrimEnd())
    $lines.Add("")
}

RunStep 'omen kbd (raw backlight bytes)' @('kbd')
RunStep 'omen power (baseline)' @('power')

RunStep 'raw 0x37 PL4=180 (B4 00)' @('raw', '0x37', '0', 'B4', '00')
RunStep 'power after 0x37' @('power')
RunStep 'raw 0x37 PL4=200 restore (C8 00)' @('raw', '0x37', '0', 'C8', '00')
RunStep 'power after restore' @('power')

RunStep 'raw 0x29 PL1=100/PL2=100 (retest)' @('raw', '0x29', '0', '64', '64', 'FF', 'FF')
RunStep 'power after 0x29' @('power')
RunStep 'raw 0x29 restore 130/130' @('raw', '0x29', '0', '82', '82', 'FF', 'FF')
RunStep 'power after 0x29 restore' @('power')

$lines -join "`r`n" | Out-File -FilePath $out -Encoding utf8
"done: $((Get-Date).ToString('HH:mm:ss'))"

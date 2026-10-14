$exe = 'C:\Users\jtoma\omen-rs\target\debug\omen.exe'
$out = 'C:\Users\jtoma\omen-rs\live-test-result.txt'
$lines = New-Object System.Collections.Generic.List[string]

function RunStep([string]$title, [string[]]$arguments) {
    $lines.Add("=== $title ===")
    $result = & $exe @arguments 2>&1 | Out-String
    $lines.Add($result.TrimEnd())
    $lines.Add("")
}

RunStep 'kbd before' @('kbd')

RunStep 'backlight OFF (regarde le clavier)' @('backlight', 'off')
Start-Sleep -Seconds 6

RunStep 'backlight ON (regarde)' @('backlight', 'on')
Start-Sleep -Seconds 6

RunStep 'colors RED (regarde 10 s)' @('color4', 'FF0000:FF0000:FF0000:FF0000')
Start-Sleep -Seconds 10

RunStep 'colors restore' @('color4', '0F84FA:710FFA:F9350F:FAAC0F')
Start-Sleep -Seconds 3
RunStep 'kbd final' @('kbd')

$lines -join "`r`n" | Out-File -FilePath $out -Encoding utf8
"done"

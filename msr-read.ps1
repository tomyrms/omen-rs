$ErrorActionPreference = 'Continue'
$out = 'C:\Users\jtoma\omen-rs\msr-read-result.txt'
$lines = New-Object System.Collections.Generic.List[string]

Add-Type -Namespace KM -Name N -MemberDefinition @'
[DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
public static extern IntPtr CreateFileW(string name, uint access, uint share, IntPtr sec, uint disp, uint flags, IntPtr tmpl);
[DllImport("kernel32.dll", SetLastError=true)]
public static extern bool DeviceIoControl(IntPtr h, uint code, ref ulong inBuf, uint inSize, ref ulong outBuf, uint outSize, out uint returned, IntPtr ov);
[DllImport("kernel32.dll", SetLastError=true)]
public static extern bool CloseHandle(IntPtr h);
'@

# take the same bus mutex the vendor client uses
$mutex = $null
try { $mutex = New-Object System.Threading.Mutex($false, 'Global\Access_PCI'); $null = $mutex.WaitOne(3000) } catch { }
$lines.Add("mutex: $($mutex -ne $null)")

$h = [KM.N]::CreateFileW('\\.\HpReadHWData', [uint32]3221225472, [uint32]0, [IntPtr]::Zero, [uint32]2, [uint32]0x80, [IntPtr]::Zero)
$hv = $h.ToInt64()
if ($hv -eq -1 -or $hv -eq 0) {
    $lines.Add("OPEN FAILED err=$([Runtime.InteropServices.Marshal]::GetLastWin32Error())")
    $lines -join "`r`n" | Out-File $out -Encoding utf8
    if ($mutex) { try { $mutex.ReleaseMutex() } catch {}; $mutex.Dispose() }
    "open failed"
    exit 1
}
$lines.Add("OPEN OK (handle $hv)")

$IOCTL_MSR = [uint32]0x9C412408

function Read-Msr([uint64]$index, [string]$label) {
    $in = [uint64]$index
    $outv = [uint64]0
    $ret = [uint32]0
    $ok = [KM.N]::DeviceIoControl($h, $IOCTL_MSR, [ref]$in, 8, [ref]$outv, 8, [ref]$ret, [IntPtr]::Zero)
    if ($ok) {
        $lines.Add(("MSR 0x{0:X3} {1,-22} = 0x{2:X16}  (hi=0x{3:X8} lo=0x{4:X8})" -f $index, $label, $outv, ($outv -shr 32), ($outv -band 0xFFFFFFFF)))
    } else {
        $lines.Add(("MSR 0x{0:X3} {1,-22} FAILED err={2}" -f $index, $label, [Runtime.InteropServices.Marshal]::GetLastWin32Error()))
    }
}

Read-Msr 0x610 'MSR_PKG_POWER_LIMIT'
Read-Msr 0x611 'MSR_PKG_ENERGY_STATUS'
Read-Msr 0x639 'MSR_PKG_POWER_INFO'
Read-Msr 0x64D 'MSR_PLATFORM_POWER_LIMIT'
Read-Msr 0x19C 'IA32_THERM_STATUS'
Read-Msr 0x1A2 'MSR_TEMPERATURE_TARGET'
Read-Msr 0x770 'IA32_PACKAGE_THERM_STATUS'
Read-Msr 0x771 'IA32_PKG_THERM_INTERRUPT'
Read-Msr 0x150 'MSR_OC_MAILBOX (undervolt?)'

[KM.N]::CloseHandle($h) | Out-Null
if ($mutex) { try { $mutex.ReleaseMutex() } catch {}; $mutex.Dispose() }
$lines.Add('done')
$lines -join "`r`n" | Out-File $out -Encoding utf8
"done"
